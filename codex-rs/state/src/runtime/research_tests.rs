use super::*;
use crate::runtime::test_support::unique_temp_dir;
use pretty_assertions::assert_eq;

fn upsert(id: &str, statement: &str) -> ResearchDelta {
    ResearchDelta {
        operation: ResearchOperation::Upsert,
        id: id.to_string(),
        scope: ResearchScope::Project,
        kind: Some(ResearchEntryKind::Hypothesis),
        subject: Some("runtime".to_string()),
        statement: Some(statement.to_string()),
        status: Some(ResearchStatus::Open),
    }
}

#[tokio::test]
async fn aliases_bridge_identity_changes() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let first = runtime
        .resolve_research_project(&["git_remote:one".to_string(), "path:stable".to_string()])
        .await?;
    let second = runtime
        .resolve_research_project(&["path:stable".to_string(), "git_remote:two".to_string()])
        .await?;
    let moved = runtime
        .resolve_research_project(&["git_remote:two".to_string(), "path:moved".to_string()])
        .await?;
    let aliases = sqlx::query_scalar::<_, String>(
        "SELECT alias FROM research_project_aliases WHERE project_id = ? ORDER BY alias",
    )
    .bind(first.project_id.as_str())
    .fetch_all(runtime.pool.as_ref())
    .await?;

    assert_eq!(second.project_id, first.project_id);
    assert_eq!(moved.project_id, first.project_id);
    assert_eq!(
        aliases,
        vec![
            "git_remote:two".to_string(),
            "path:moved".to_string(),
            "path:stable".to_string(),
        ]
    );
    runtime.close().await;
    Ok(())
}

#[tokio::test]
async fn project_deltas_persist_without_idempotent_event_churn() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let project = runtime
        .resolve_research_project(&["remote:project".to_string()])
        .await?;
    let delta = upsert("phase3", "Project research should survive sessions");

    let changed = runtime
        .apply_research_project_deltas(project.project_id.as_str(), std::slice::from_ref(&delta))
        .await?;
    let supported = ResearchDelta {
        operation: ResearchOperation::SetStatus,
        id: "phase3".to_string(),
        scope: ResearchScope::Project,
        kind: None,
        subject: None,
        statement: None,
        status: Some(ResearchStatus::Supported),
    };
    let supported_update = runtime
        .apply_research_project_deltas(
            project.project_id.as_str(),
            std::slice::from_ref(&supported),
        )
        .await?;
    let unchanged = runtime
        .apply_research_project_deltas(
            project.project_id.as_str(),
            std::slice::from_ref(&supported),
        )
        .await?;
    let snapshot = runtime
        .load_research_project_snapshot(project.project_id.as_str())
        .await?;
    let event_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM research_events WHERE project_id = ?")
            .bind(project.project_id.as_str())
            .fetch_one(runtime.pool.as_ref())
            .await?;
    let event_rows = sqlx::query_as::<_, (i64, String)>(
        r#"
SELECT revision, payload_json
FROM research_events
WHERE project_id = ?
ORDER BY revision, event_index
        "#,
    )
    .bind(project.project_id.as_str())
    .fetch_all(runtime.pool.as_ref())
    .await?;
    let mut replayed = ResearchState::default();
    let mut replay_revision = None;
    let mut replay_batch = Vec::new();
    for (revision, payload_json) in event_rows {
        if replay_revision.is_some_and(|current| current != revision) {
            replayed.apply(&replay_batch)?;
            replay_batch.clear();
        }
        replay_revision = Some(revision);
        replay_batch.push(serde_json::from_str::<ResearchDelta>(&payload_json)?);
    }
    if !replay_batch.is_empty() {
        replayed.apply(&replay_batch)?;
    }

    assert_eq!(
        (
            changed.revision,
            changed.changed,
            supported_update.revision,
            supported_update.changed,
            unchanged.revision,
            unchanged.changed
        ),
        (1, true, 2, true, 2, false)
    );
    assert_eq!(
        snapshot,
        ResearchStateSnapshot {
            revision: supported_update.revision,
            entries: supported_update.entries,
        }
    );
    assert_eq!(replayed.snapshot(), snapshot);
    assert_eq!(event_count, 2);
    runtime.close().await;
    Ok(())
}
