use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

// Types for the native TODO/checklist tool. The base plan fields remain compatible with
// codex-vscode/todo-mcp, while optional research deltas are handled by Codex itself.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct PlanItemArg {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlanArgs {
    /// Arguments for the `update_plan` todo/checklist tool (not plan mode).
    #[serde(default)]
    pub explanation: Option<String>,
    pub plan: Vec<PlanItemArg>,
    /// Optional append-only research state changes associated with this plan update.
    #[serde(default)]
    pub research_delta: Option<Vec<ResearchDelta>>,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ResearchScope {
    Task,
    Project,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchEntryKind {
    Fact,
    Constraint,
    Goal,
    Unknown,
    Hypothesis,
    Exploration,
    Decision,
    FuturePressure,
    Outcome,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Open,
    Supported,
    Rejected,
    Resolved,
    Superseded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchOperation {
    Upsert,
    SetStatus,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchDelta {
    pub operation: ResearchOperation,
    pub id: String,
    pub scope: ResearchScope,
    #[serde(default)]
    pub kind: Option<ResearchEntryKind>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub statement: Option<String>,
    #[serde(default)]
    pub status: Option<ResearchStatus>,
}

/// Current materialized value of one research-state entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchEntry {
    pub id: String,
    pub scope: ResearchScope,
    pub kind: ResearchEntryKind,
    pub subject: String,
    pub statement: String,
    pub status: ResearchStatus,
}

/// Read-only materialized research state for a session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchStateSnapshot {
    pub revision: u64,
    pub entries: Vec<ResearchEntry>,
}

/// Result of transactionally applying a research-state delta batch.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchStateUpdate {
    pub revision: u64,
    pub changed: bool,
    pub entries: Vec<ResearchEntry>,
}
