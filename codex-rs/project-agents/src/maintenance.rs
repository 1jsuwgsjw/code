use crate::PROJECT_AGENT_SCHEMA_VERSION;
use crate::ProjectAgentId;
use crate::RelativeProjectAgentPath;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAgentMaintenanceItemKind {
    MemoryCandidate,
    ImprovementProposal,
}

impl ProjectAgentMaintenanceItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MemoryCandidate => "memory_candidate",
            Self::ImprovementProposal => "improvement_proposal",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAgentMaintenanceDisposition {
    Accepted,
    Rejected,
}

impl ProjectAgentMaintenanceDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAgentMaintenanceMode {
    Apply,
    DryRun,
}

impl ProjectAgentMaintenanceMode {
    pub fn is_dry_run(self) -> bool {
        self == Self::DryRun
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectAgentMaintenanceTarget {
    All,
    Agent(ProjectAgentId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAgentMaintenanceOptions {
    pub actor: String,
    pub mode: ProjectAgentMaintenanceMode,
}

impl ProjectAgentMaintenanceOptions {
    pub fn apply(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            mode: ProjectAgentMaintenanceMode::Apply,
        }
    }

    pub fn dry_run(actor: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            mode: ProjectAgentMaintenanceMode::DryRun,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentPendingCounts {
    pub memory_candidates: u64,
    pub improvement_proposals: u64,
}

impl ProjectAgentPendingCounts {
    pub fn total(self) -> u64 {
        self.memory_candidates
            .saturating_add(self.improvement_proposals)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMaintenanceAgentStatus {
    pub agent_id: ProjectAgentId,
    pub pending: ProjectAgentPendingCounts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMaintenanceStatus {
    pub catalog_revision: u64,
    pub agents: Vec<ProjectAgentMaintenanceAgentStatus>,
}

impl ProjectAgentMaintenanceStatus {
    pub fn pending(&self) -> ProjectAgentPendingCounts {
        self.agents
            .iter()
            .fold(ProjectAgentPendingCounts::default(), |mut total, agent| {
                total.memory_candidates = total
                    .memory_candidates
                    .saturating_add(agent.pending.memory_candidates);
                total.improvement_proposals = total
                    .improvement_proposals
                    .saturating_add(agent.pending.improvement_proposals);
                total
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMaintenanceDecision {
    pub schema_version: u32,
    pub maintenance_id: String,
    pub item_id: String,
    pub kind: ProjectAgentMaintenanceItemKind,
    pub disposition: ProjectAgentMaintenanceDisposition,
    pub agent_id: ProjectAgentId,
    pub source_task_id: String,
    pub source_path: RelativeProjectAgentPath,
    pub body: String,
    pub evidence: Vec<String>,
    pub reason: String,
    pub actor: String,
    pub decided_at_ms: i64,
    pub decision_path: RelativeProjectAgentPath,
    pub accepted_path: Option<RelativeProjectAgentPath>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMaintenanceReport {
    pub schema_version: u32,
    pub maintenance_id: String,
    pub agent_id: ProjectAgentId,
    pub actor: String,
    pub mode: ProjectAgentMaintenanceMode,
    pub started_at_ms: i64,
    pub completed_at_ms: i64,
    pub catalog_revision_before: u64,
    pub catalog_revision_after: u64,
    pub pending_before: ProjectAgentPendingCounts,
    pub pending_after: ProjectAgentPendingCounts,
    pub decisions: Vec<ProjectAgentMaintenanceDecision>,
}

impl ProjectAgentMaintenanceReport {
    pub fn accepted_count(&self) -> usize {
        self.decisions
            .iter()
            .filter(|decision| decision.disposition == ProjectAgentMaintenanceDisposition::Accepted)
            .count()
    }

    pub fn rejected_count(&self) -> usize {
        self.decisions.len().saturating_sub(self.accepted_count())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMaintenanceOutcome {
    pub mode: ProjectAgentMaintenanceMode,
    pub reports: Vec<ProjectAgentMaintenanceReport>,
    pub status: ProjectAgentMaintenanceStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct PendingProjectAgentItem {
    pub(crate) schema_version: u32,
    pub(crate) agent_id: ProjectAgentId,
    pub(crate) task_id: String,
    pub(crate) body: String,
}

impl PendingProjectAgentItem {
    pub(crate) fn new(agent_id: ProjectAgentId, task_id: String, body: String) -> Self {
        Self {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            agent_id,
            task_id,
            body,
        }
    }
}
