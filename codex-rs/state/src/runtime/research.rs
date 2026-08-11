use super::StateRuntime;
use codex_protocol::plan_tool::ResearchDelta;
use codex_protocol::plan_tool::ResearchEntry;
use codex_protocol::plan_tool::ResearchEntryKind;
use codex_protocol::plan_tool::ResearchOperation;
use codex_protocol::plan_tool::ResearchScope;
use codex_protocol::plan_tool::ResearchStateSnapshot;
use codex_protocol::plan_tool::ResearchStateUpdate;
use codex_protocol::plan_tool::ResearchStatus;
use codex_research_state::ResearchState;
use sqlx::Row;
use sqlx::SqliteConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_PROJECT_ALIASES: usize = 32;
const MAX_PROJECT_ALIAS_LEN: usize = 256;
const RESEARCH_PROJECT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x91, 0x76, 0x5d, 0x2f, 0x5b, 0x1e, 0x4e, 0x9c, 0x91, 0x21, 0x7d, 0x90, 0xd4, 0x52, 0x39, 0x83,
]);

/// Stable internal research project resolved from one or more opaque aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResearchProject {
    pub project_id: String,
    pub revision: u64,
}

impl StateRuntime {
    /// Resolve durable project identity and attach all newly observed aliases.
    pub async fn resolve_research_project(
        &self,
        aliases: &[String],
    ) -> anyhow::Result<ResearchProject> {
        let aliases = normalize_aliases(aliases)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut project_ids = BTreeSet::new();
        for alias in &aliases {
            let project_id = sqlx::query_scalar::<_, String>(
                "SELECT project_id FROM research_project_aliases WHERE alias = ?",
            )
            .bind(alias)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(project_id) = project_id {
                project_ids.insert(project_id);
            }
        }
        anyhow::ensure!(
            project_ids.len() <= 1,
            "research project aliases resolve to conflicting projects"
        );

        let now = chrono::Utc::now().timestamp_millis();
        let project_id = project_ids.into_iter().next().unwrap_or_else(|| {
            format!(
                "research-{}",
                Uuid::new_v5(&RESEARCH_PROJECT_NAMESPACE, aliases[0].as_bytes())
            )
        });
        sqlx::query(
            r#"
INSERT INTO research_projects (project_id, revision, created_at_ms, updated_at_ms)
VALUES (?, 0, ?, ?)
ON CONFLICT(project_id) DO NOTHING
            "#,
        )
        .bind(project_id.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        // Remote and root-commit aliases are strong identity evidence for the
        // current checkout. Replace stale values so a removed upstream or an
        // old repository fingerprint cannot merge an unrelated future project.
        sqlx::query(
            r#"
DELETE FROM research_project_aliases
WHERE project_id = ?
  AND (alias GLOB 'git_remote:*' OR alias GLOB 'git_roots:*')
            "#,
        )
        .bind(project_id.as_str())
        .execute(&mut *tx)
        .await?;

        for alias in &aliases {
            sqlx::query(
                r#"
INSERT INTO research_project_aliases (alias, project_id, created_at_ms)
VALUES (?, ?, ?)
ON CONFLICT(alias) DO NOTHING
                "#,
            )
            .bind(alias)
            .bind(project_id.as_str())
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        let revision = load_project_revision(&mut tx, project_id.as_str()).await?;
        tx.commit().await?;
        Ok(ResearchProject {
            project_id,
            revision,
        })
    }

    pub async fn load_research_project_snapshot(
        &self,
        project_id: &str,
    ) -> anyhow::Result<ResearchStateSnapshot> {
        let mut tx = self.pool.begin().await?;
        let snapshot = load_project_snapshot(&mut tx, project_id).await?;
        tx.commit().await?;
        Ok(snapshot)
    }

    /// Apply project deltas with the domain reducer and persist events,
    /// projection, and revision in one SQLite transaction.
    pub async fn apply_research_project_deltas(
        &self,
        project_id: &str,
        deltas: &[ResearchDelta],
    ) -> anyhow::Result<ResearchStateUpdate> {
        for delta in deltas {
            anyhow::ensure!(
                delta.scope == ResearchScope::Project,
                "persistent research delta must use project scope"
            );
        }

        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let snapshot = load_project_snapshot(&mut tx, project_id).await?;
        let previous_revision = snapshot.revision;
        let mut state = ResearchState::from_snapshot(snapshot)?;
        let update = state.apply(deltas)?;
        if !update.changed {
            tx.commit().await?;
            return Ok(update);
        }

        let revision = i64::try_from(update.revision)
            .map_err(|_| anyhow::anyhow!("research revision exceeds SQLite range"))?;
        let previous_revision = i64::try_from(previous_revision)
            .map_err(|_| anyhow::anyhow!("research revision exceeds SQLite range"))?;
        let now = chrono::Utc::now().timestamp_millis();
        let result = sqlx::query(
            r#"
UPDATE research_projects
SET revision = ?, updated_at_ms = ?
WHERE project_id = ? AND revision = ?
            "#,
        )
        .bind(revision)
        .bind(now)
        .bind(project_id)
        .bind(previous_revision)
        .execute(&mut *tx)
        .await?;
        anyhow::ensure!(
            result.rows_affected() == 1,
            "research project revision changed during update"
        );

        for (event_index, delta) in deltas.iter().enumerate() {
            let event_index = i64::try_from(event_index)
                .map_err(|_| anyhow::anyhow!("research delta batch is too large"))?;
            sqlx::query(
                r#"
INSERT INTO research_events (
    project_id, revision, event_index, operation, entry_id, scope, payload_json, created_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(project_id)
            .bind(revision)
            .bind(event_index)
            .bind(operation_name(delta.operation))
            .bind(delta.id.trim())
            .bind(scope_name(delta.scope))
            .bind(serde_json::to_string(delta)?)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        let affected_ids = deltas
            .iter()
            .map(|delta| delta.id.trim().to_string())
            .collect::<BTreeSet<_>>();
        for entry_id in affected_ids {
            if let Some(entry) = update
                .entries
                .iter()
                .find(|entry| entry.scope == ResearchScope::Project && entry.id == entry_id)
            {
                upsert_projection(&mut tx, project_id, revision, now, entry).await?;
            } else {
                sqlx::query(
                    "DELETE FROM research_projections WHERE project_id = ? AND scope = ? AND entry_id = ?",
                )
                .bind(project_id)
                .bind(scope_name(ResearchScope::Project))
                .bind(entry_id)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(update)
    }
}

fn normalize_aliases(aliases: &[String]) -> anyhow::Result<Vec<String>> {
    let aliases = aliases
        .iter()
        .map(|alias| alias.trim())
        .filter(|alias| !alias.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !aliases.is_empty(),
        "research project requires an identity alias"
    );
    anyhow::ensure!(
        aliases.len() <= MAX_PROJECT_ALIASES,
        "research project has too many identity aliases"
    );
    anyhow::ensure!(
        aliases
            .iter()
            .all(|alias| alias.len() <= MAX_PROJECT_ALIAS_LEN),
        "research project identity alias is too long"
    );
    Ok(aliases)
}

async fn load_project_revision(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> anyhow::Result<u64> {
    let revision =
        sqlx::query_scalar::<_, i64>("SELECT revision FROM research_projects WHERE project_id = ?")
            .bind(project_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("research project `{project_id}` does not exist"))?;
    u64::try_from(revision).map_err(|_| anyhow::anyhow!("research project has invalid revision"))
}

async fn load_project_snapshot(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> anyhow::Result<ResearchStateSnapshot> {
    let revision = load_project_revision(connection, project_id).await?;
    let rows = sqlx::query(
        r#"
SELECT payload_json
FROM research_projections
WHERE project_id = ?
ORDER BY scope ASC, entry_id ASC
        "#,
    )
    .bind(project_id)
    .fetch_all(&mut *connection)
    .await?;
    let entries = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<ResearchEntry>(row.get::<String, _>("payload_json").as_str())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ResearchStateSnapshot { revision, entries })
}

async fn upsert_projection(
    connection: &mut SqliteConnection,
    project_id: &str,
    revision: i64,
    now: i64,
    entry: &ResearchEntry,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
INSERT INTO research_projections (
    project_id, scope, entry_id, kind, subject, statement, status, payload_json, revision,
    updated_at_ms
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(project_id, scope, entry_id) DO UPDATE SET
    kind = excluded.kind,
    subject = excluded.subject,
    statement = excluded.statement,
    status = excluded.status,
    payload_json = excluded.payload_json,
    revision = excluded.revision,
    updated_at_ms = excluded.updated_at_ms
        "#,
    )
    .bind(project_id)
    .bind(scope_name(entry.scope))
    .bind(entry.id.as_str())
    .bind(kind_name(entry.kind))
    .bind(entry.subject.as_str())
    .bind(entry.statement.as_str())
    .bind(status_name(entry.status))
    .bind(serde_json::to_string(entry)?)
    .bind(revision)
    .bind(now)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

fn operation_name(operation: ResearchOperation) -> &'static str {
    match operation {
        ResearchOperation::Upsert => "upsert",
        ResearchOperation::SetStatus => "set_status",
        ResearchOperation::Remove => "remove",
    }
}

fn scope_name(scope: ResearchScope) -> &'static str {
    match scope {
        ResearchScope::Task => "task",
        ResearchScope::Project => "project",
    }
}

fn kind_name(kind: ResearchEntryKind) -> &'static str {
    match kind {
        ResearchEntryKind::Fact => "fact",
        ResearchEntryKind::Constraint => "constraint",
        ResearchEntryKind::Goal => "goal",
        ResearchEntryKind::Unknown => "unknown",
        ResearchEntryKind::Hypothesis => "hypothesis",
        ResearchEntryKind::Exploration => "exploration",
        ResearchEntryKind::Decision => "decision",
        ResearchEntryKind::FuturePressure => "future_pressure",
        ResearchEntryKind::Outcome => "outcome",
    }
}

fn status_name(status: ResearchStatus) -> &'static str {
    match status {
        ResearchStatus::Open => "open",
        ResearchStatus::Supported => "supported",
        ResearchStatus::Rejected => "rejected",
        ResearchStatus::Resolved => "resolved",
        ResearchStatus::Superseded => "superseded",
    }
}

#[cfg(test)]
#[path = "research_tests.rs"]
mod tests;
