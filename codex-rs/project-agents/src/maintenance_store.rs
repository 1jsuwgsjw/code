use crate::PROJECT_AGENT_SCHEMA_VERSION;
use crate::ProjectAgentDefinition;
use crate::ProjectAgentEntry;
use crate::ProjectAgentFileSystemScope;
use crate::ProjectAgentId;
use crate::ProjectAgentMaintenanceAgentStatus;
use crate::ProjectAgentMaintenanceDecision;
use crate::ProjectAgentMaintenanceDisposition;
use crate::ProjectAgentMaintenanceItemKind;
use crate::ProjectAgentMaintenanceOptions;
use crate::ProjectAgentMaintenanceOutcome;
use crate::ProjectAgentMaintenanceReport;
use crate::ProjectAgentMaintenanceStatus;
use crate::ProjectAgentMaintenanceTarget;
use crate::ProjectAgentMemoryIndex;
use crate::ProjectAgentPendingCounts;
use crate::ProjectAgentRegistry;
use crate::ProjectAgentStore;
use crate::ProjectAgentStoreError;
use crate::ProjectAgentTaskResult;
use crate::ProjectAgentTaskStatus;
use crate::RelativeProjectAgentPath;
use crate::maintenance::PendingProjectAgentItem;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::RemoveOptions;
use codex_utils_path_uri::PathUri;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
mod error;
mod pending;
mod persistence;

pub use error::ProjectAgentMaintenanceError;

const MAX_ACTOR_BYTES: usize = 256;
const MAX_PENDING_ITEM_BYTES: usize = 16 * 1024;
const MAX_TASK_RESULT_BYTES: usize = 256 * 1024;
const MAX_DECISION_BYTES: usize = 128 * 1024;
const MAX_REPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_MEMORY_INDEX_BYTES: usize = 64 * 1024;
const MAX_MEMORY_ITEM_BYTES: usize = 32 * 1024;
const MAX_PENDING_DIRECTORY_ENTRIES: usize = 10_000;
const MAX_PENDING_ITEMS_PER_KIND: usize = 1_024;
const APPROX_BYTES_PER_TOKEN: usize = 4;

static MAINTENANCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
struct PendingItemFile {
    item_id: String,
    source_path: RelativeProjectAgentPath,
    item: PendingProjectAgentItem,
}

#[derive(Debug)]
struct MemoryState {
    index: ProjectAgentMemoryIndex,
    normalized: BTreeMap<String, RelativeProjectAgentPath>,
    used_tokens: usize,
}

impl ProjectAgentStore {
    pub async fn maintenance_status(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        target: &ProjectAgentMaintenanceTarget,
    ) -> Result<ProjectAgentMaintenanceStatus, ProjectAgentMaintenanceError> {
        let registry = self.load(file_system, scope).await?;
        let entries = self.maintenance_entries(file_system, scope, target).await?;
        let mut agents = Vec::with_capacity(entries.len());
        for entry in entries {
            agents.push(ProjectAgentMaintenanceAgentStatus {
                pending: self
                    .pending_counts(file_system, scope, &entry.definition.id)
                    .await?,
                agent_id: entry.definition.id,
            });
        }
        Ok(ProjectAgentMaintenanceStatus {
            catalog_revision: registry.revision,
            agents,
        })
    }

    pub async fn maintain(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        target: ProjectAgentMaintenanceTarget,
        options: ProjectAgentMaintenanceOptions,
    ) -> Result<ProjectAgentMaintenanceOutcome, ProjectAgentMaintenanceError> {
        let actor = options.actor.trim();
        if actor.is_empty() || actor.len() > MAX_ACTOR_BYTES {
            return Err(ProjectAgentMaintenanceError::InvalidActor);
        }

        let entries = self
            .maintenance_entries(file_system, scope, &target)
            .await?;
        let mut reports = Vec::with_capacity(entries.len());
        for entry in entries {
            reports.push(
                self.maintain_agent(file_system, scope, &entry, actor, options.mode)
                    .await?,
            );
        }
        let status = self.maintenance_status(file_system, scope, &target).await?;
        Ok(ProjectAgentMaintenanceOutcome {
            mode: options.mode,
            reports,
            status,
        })
    }

    async fn maintenance_entries(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        target: &ProjectAgentMaintenanceTarget,
    ) -> Result<Vec<ProjectAgentEntry>, ProjectAgentMaintenanceError> {
        match target {
            ProjectAgentMaintenanceTarget::All => Ok(self.list(file_system, scope).await?),
            ProjectAgentMaintenanceTarget::Agent(agent_id) => {
                Ok(vec![self.get(file_system, scope, agent_id).await?])
            }
        }
    }

    async fn maintain_agent(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        entry: &ProjectAgentEntry,
        actor: &str,
        mode: crate::ProjectAgentMaintenanceMode,
    ) -> Result<ProjectAgentMaintenanceReport, ProjectAgentMaintenanceError> {
        let agent_id = &entry.definition.id;
        let started_at_ms = unix_time_ms()?;
        let maintenance_id = format!(
            "maintenance-{started_at_ms}-{}-{}",
            std::process::id(),
            MAINTENANCE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let lock_directory = self
            .resolve_agent_relative(agent_id, "maintenance/.lock")?
            .1;
        self.acquire_maintenance_lock(
            file_system,
            scope,
            agent_id,
            actor,
            &maintenance_id,
            started_at_ms,
            &lock_directory,
        )
        .await?;

        let result = self
            .maintain_agent_locked(
                file_system,
                scope,
                entry,
                actor,
                mode,
                maintenance_id,
                started_at_ms,
            )
            .await;
        let release_result = file_system
            .remove(
                &lock_directory,
                RemoveOptions {
                    recursive: true,
                    force: true,
                },
                sandbox(scope),
            )
            .await
            .map_err(|source| self.fs_error("release maintenance lock", &lock_directory, source));
        match (result, release_result) {
            (Ok(report), Ok(())) => Ok(report),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn maintain_agent_locked(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        entry: &ProjectAgentEntry,
        actor: &str,
        mode: crate::ProjectAgentMaintenanceMode,
        maintenance_id: String,
        started_at_ms: i64,
    ) -> Result<ProjectAgentMaintenanceReport, ProjectAgentMaintenanceError> {
        let agent_id = &entry.definition.id;
        let memory_items = self
            .pending_items(
                file_system,
                scope,
                agent_id,
                ProjectAgentMaintenanceItemKind::MemoryCandidate,
            )
            .await?;
        let proposal_items = self
            .pending_items(
                file_system,
                scope,
                agent_id,
                ProjectAgentMaintenanceItemKind::ImprovementProposal,
            )
            .await?;
        let pending_before = ProjectAgentPendingCounts {
            memory_candidates: memory_items.len() as u64,
            improvement_proposals: proposal_items.len() as u64,
        };
        let registry = self.load(file_system, scope).await?;
        let catalog_revision_before = registry.revision;
        let mut memory_state = self.load_memory_state(file_system, scope, agent_id).await?;
        let mut accepted_proposals = self
            .load_accepted_proposals(file_system, scope, agent_id)
            .await?;
        let mut decisions = Vec::with_capacity(memory_items.len() + proposal_items.len());
        let mut new_memory = Vec::<(RelativeProjectAgentPath, String)>::new();

        for pending in memory_items {
            let (evidence, source_reason) = self
                .source_evidence(
                    file_system,
                    scope,
                    agent_id,
                    &pending,
                    ProjectAgentMaintenanceItemKind::MemoryCandidate,
                )
                .await?;
            let normalized = normalize_body(&pending.item.body);
            let fact_path =
                RelativeProjectAgentPath::new(format!("memory/facts/{}.md", pending.item_id))?;
            let existing_fact = self
                .read_optional_text(
                    file_system,
                    scope,
                    agent_id,
                    &fact_path,
                    MAX_MEMORY_ITEM_BYTES,
                )
                .await?;
            let (disposition, reason, accepted_path) = if let Some(reason) = source_reason {
                (ProjectAgentMaintenanceDisposition::Rejected, reason, None)
            } else if existing_fact
                .as_deref()
                .is_some_and(|body| normalize_body(body) == normalized)
            {
                if !memory_state.index.items.contains(&fact_path) {
                    memory_state.index.items.push(fact_path.clone());
                }
                memory_state
                    .normalized
                    .insert(normalized.clone(), fact_path.clone());
                (
                    ProjectAgentMaintenanceDisposition::Accepted,
                    "recovered an already materialized accepted-memory fact".to_string(),
                    Some(fact_path),
                )
            } else if existing_fact.is_some() {
                return Err(ProjectAgentMaintenanceError::AcceptedPathConflict(
                    self.resolve_agent_path(agent_id, &fact_path)?,
                ));
            } else if memory_state.normalized.contains_key(&normalized) {
                (
                    ProjectAgentMaintenanceDisposition::Rejected,
                    "duplicates accepted memory".to_string(),
                    None,
                )
            } else {
                let next_items = memory_state.index.items.len().saturating_add(1);
                let next_tokens = memory_state
                    .used_tokens
                    .saturating_add(approx_tokens(&pending.item.body));
                if next_items
                    > usize::try_from(entry.definition.memory_max_items).unwrap_or(usize::MAX)
                    || next_tokens
                        > usize::try_from(entry.definition.memory_max_tokens).unwrap_or(usize::MAX)
                {
                    (
                        ProjectAgentMaintenanceDisposition::Rejected,
                        "accepted-memory capacity would be exceeded".to_string(),
                        None,
                    )
                } else {
                    memory_state.index.items.push(fact_path.clone());
                    memory_state
                        .normalized
                        .insert(normalized, fact_path.clone());
                    memory_state.used_tokens = next_tokens;
                    new_memory.push((fact_path.clone(), pending.item.body.clone()));
                    (
                        ProjectAgentMaintenanceDisposition::Accepted,
                        "validated completed source task, evidence, uniqueness, and capacity"
                            .to_string(),
                        Some(fact_path),
                    )
                }
            };
            decisions.push(self.decision(
                agent_id,
                actor,
                &maintenance_id,
                pending,
                ProjectAgentMaintenanceItemKind::MemoryCandidate,
                disposition,
                evidence,
                reason,
                accepted_path,
                started_at_ms,
            )?);
        }

        for pending in proposal_items {
            let (evidence, source_reason) = self
                .source_evidence(
                    file_system,
                    scope,
                    agent_id,
                    &pending,
                    ProjectAgentMaintenanceItemKind::ImprovementProposal,
                )
                .await?;
            let normalized = normalize_body(&pending.item.body);
            let (disposition, reason) = if let Some(reason) = source_reason {
                (ProjectAgentMaintenanceDisposition::Rejected, reason)
            } else if !accepted_proposals.insert(normalized) {
                (
                    ProjectAgentMaintenanceDisposition::Rejected,
                    "duplicates an accepted improvement proposal".to_string(),
                )
            } else {
                (
                    ProjectAgentMaintenanceDisposition::Accepted,
                    "validated completed source task, evidence, and uniqueness".to_string(),
                )
            };
            decisions.push(self.decision(
                agent_id,
                actor,
                &maintenance_id,
                pending,
                ProjectAgentMaintenanceItemKind::ImprovementProposal,
                disposition,
                evidence,
                reason,
                None,
                started_at_ms,
            )?);
        }

        let mut catalog_revision_after = catalog_revision_before;
        if !mode.is_dry_run() {
            self.apply_decisions(
                file_system,
                scope,
                agent_id,
                &memory_state.index,
                &new_memory,
                &decisions,
            )
            .await?;
            if !decisions.is_empty() {
                catalog_revision_after =
                    self.increment_catalog_revision(file_system, scope).await?;
            }
        }
        let pending_after = if mode.is_dry_run() {
            pending_before
        } else {
            ProjectAgentPendingCounts::default()
        };
        let completed_at_ms = unix_time_ms()?;
        let report = ProjectAgentMaintenanceReport {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            maintenance_id,
            agent_id: agent_id.clone(),
            actor: actor.to_string(),
            mode,
            started_at_ms,
            completed_at_ms,
            catalog_revision_before,
            catalog_revision_after,
            pending_before,
            pending_after,
            decisions,
        };
        if !mode.is_dry_run() {
            self.persist_report(file_system, scope, &report).await?;
        }
        Ok(report)
    }
}

fn sandbox(scope: ProjectAgentFileSystemScope<'_>) -> Option<&FileSystemSandboxContext> {
    match scope {
        ProjectAgentFileSystemScope::Unrestricted => None,
        ProjectAgentFileSystemScope::Sandboxed(sandbox) => Some(sandbox),
    }
}

fn stable_item_id(
    kind: ProjectAgentMaintenanceItemKind,
    source_path: &RelativeProjectAgentPath,
) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in kind
        .as_str()
        .bytes()
        .chain([b':'])
        .chain(source_path.as_str().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let prefix = match kind {
        ProjectAgentMaintenanceItemKind::MemoryCandidate => "memory",
        ProjectAgentMaintenanceItemKind::ImprovementProposal => "proposal",
    };
    format!("{prefix}-{hash:016x}")
}

fn normalize_body(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn approx_tokens(body: &str) -> usize {
    body.len().div_ceil(APPROX_BYTES_PER_TOKEN)
}

fn unix_time_ms() -> Result<i64, ProjectAgentMaintenanceError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(ProjectAgentMaintenanceError::SystemTime)?
        .as_millis();
    i64::try_from(millis).map_err(|_| ProjectAgentMaintenanceError::TimestampOverflow)
}
