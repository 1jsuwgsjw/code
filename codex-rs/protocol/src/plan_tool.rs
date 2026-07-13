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
