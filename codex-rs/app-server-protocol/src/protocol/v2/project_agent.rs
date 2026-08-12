use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentListParams {
    pub thread_id: String,
    /// Opaque pagination cursor returned by a previous call.
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    /// Optional page size; the server applies a hard upper bound.
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentListResponse {
    pub project_root: String,
    pub data: Vec<ProjectAgentRosterEntry>,
    pub next_cursor: Option<String>,
    pub total: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentReadParams {
    pub thread_id: String,
    pub agent_id: String,
    /// Optional recent-task and result count; the server applies a hard upper bound.
    #[ts(optional = nullable)]
    pub task_limit: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentReadResponse {
    pub project_root: String,
    pub agent: ProjectAgentDefinition,
    pub active_session_thread_id: Option<String>,
    pub session: Option<ProjectAgentSession>,
    pub current_task: Option<ProjectAgentTask>,
    pub current_result: Option<ProjectAgentPersistedResult>,
    pub recent_tasks: Vec<ProjectAgentTask>,
    pub recent_results: Vec<ProjectAgentPersistedResult>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentStartParams {
    pub thread_id: String,
    pub agent_id: String,
    pub task: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentStartResponse {
    pub task: ProjectAgentTask,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentFollowUpParams {
    pub thread_id: String,
    pub agent_id: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentFollowUpResponse {
    pub task_id: String,
    pub session_thread_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentTerminateParams {
    pub thread_id: String,
    pub agent_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentTerminateResponse {
    pub task_id: String,
    pub session_thread_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentRetryParams {
    pub thread_id: String,
    pub agent_id: String,
    #[ts(optional = nullable)]
    pub task_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentRetryResponse {
    pub task: ProjectAgentTask,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentRebuildParams {
    pub thread_id: String,
    pub agent_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentRebuildResponse {
    pub previous_session_thread_id: Option<String>,
    pub session: ProjectAgentSession,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectAgentRosterEntry {
    pub id: String,
    pub description: String,
    pub enabled: bool,
    pub active_session_thread_id: Option<String>,
    pub session: Option<ProjectAgentSession>,
    pub current_task: Option<ProjectAgentTask>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectAgentDefinition {
    pub id: String,
    pub description: String,
    pub enabled: bool,
    pub definition_path: String,
    pub constraints_file: String,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub model_provider: Option<String>,
    pub tools: Vec<String>,
    pub memory_max_items: u32,
    pub memory_max_tokens: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectAgentSession {
    pub agent_id: String,
    pub thread_id: String,
    pub generation: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectAgentTask {
    pub agent_id: String,
    pub task_id: String,
    pub task: String,
    pub phase: ProjectAgentTaskPhase,
    pub session_thread_id: Option<String>,
    pub parent_thread_id: String,
    pub attempt: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum ProjectAgentTaskPhase {
    Queued,
    Running,
    Completed,
    Interrupted,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ProjectAgentPersistedResult {
    pub status: ProjectAgentTaskStatus,
    pub agent_id: String,
    pub task_id: String,
    pub result: String,
    pub artifacts: Vec<String>,
    pub evidence: Vec<String>,
    pub memory_candidates: Vec<String>,
    pub improvement_proposals: Vec<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum ProjectAgentTaskStatus {
    Completed,
    RejectedOutOfScope,
    BlockedMissingTool,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentMaintenanceRunParams {
    pub thread_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentMaintenanceRunResponse {
    pub accepted_count: u64,
    pub rejected_count: u64,
    pub status: ThreadProjectAgentMaintenanceStatusUpdatedNotification,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[schemars(rename = "ThreadProjectAgentMaintenanceStatus")]
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentMaintenanceStatusUpdatedNotification {
    pub thread_id: String,
    pub project_root: String,
    pub pending_count: u64,
    pub catalog_revision: u64,
}
