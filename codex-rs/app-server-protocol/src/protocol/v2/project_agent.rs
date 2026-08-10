use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

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
#[ts(export_to = "v2/")]
pub struct ThreadProjectAgentMaintenanceStatusUpdatedNotification {
    pub thread_id: String,
    pub project_root: String,
    pub pending_count: u64,
    pub catalog_revision: u64,
}
