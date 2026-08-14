use crate::ArtifactId;
use crate::CheckpointGenerationId;
use crate::ToolGroupId;
use crate::TurnRecordId;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub sha256: String,
    pub byte_len: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ToolCallOutcome {
    Success,
    ToolError { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TruncationProvenance {
    Complete,
    ModelFacingFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocationRecord {
    pub call_id: String,
    pub tool_name: String,
    pub media_type: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultRecord {
    pub media_type: String,
    pub payload: Vec<u8>,
    pub outcome: ToolCallOutcome,
    pub truncation: TruncationProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub call_id: String,
    pub tool_name: String,
    pub group_id: ToolGroupId,
    pub input: ArtifactRef,
    pub output: ArtifactRef,
    pub outcome: ToolCallOutcome,
    pub truncation: TruncationProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolGroupState {
    Open,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolGroupRecord {
    pub group_id: ToolGroupId,
    pub turn_id: String,
    pub history_start: usize,
    pub history_end: Option<usize>,
    pub calls: Vec<ToolCallRecord>,
    pub state: ToolGroupState,
    pub requires_recall: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EvidenceRef {
    Artifact { artifact_id: ArtifactId },
    WorkspacePath { path: String },
    Symbol { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
    pub record_id: TurnRecordId,
    pub generation_id: CheckpointGenerationId,
    pub completed_groups: Vec<ToolGroupId>,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
    pub changes: Vec<String>,
    pub validation: Vec<String>,
    pub decisions: Vec<String>,
    pub open_items: Vec<String>,
    pub next_action: String,
    pub correction_of: Option<TurnRecordId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSummaryRequest {
    #[serde(default)]
    pub completed_tool_groups: Vec<ToolGroupId>,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub changes: Vec<String>,
    #[serde(default)]
    pub validation: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub open_items: Vec<String>,
    pub next_action: String,
    #[serde(default)]
    pub correction_of: Option<TurnRecordId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCheckpoint {
    pub record: TurnRecord,
    pub history_start: usize,
    pub history_end: usize,
    pub completed_groups: Vec<ToolGroupId>,
    #[serde(skip)]
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledCheckpoint {
    pub record_id: TurnRecordId,
    pub replacement_history_sha256: String,
    pub replacement_item_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckpointPressure {
    Normal,
    Advisory,
    Required,
    FallbackRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointToolPolicy {
    Normal,
    CheckpointOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    Provider,
    TokenizerEstimate,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsageSnapshot {
    pub active_tokens: u64,
    pub context_window: Option<u64>,
    pub usage_source: UsageSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStatusSnapshot {
    pub generation_id: CheckpointGenerationId,
    pub turn_id: String,
    pub context_usage_basis_points: u16,
    pub usage_source: UsageSource,
    pub tool_result_share_basis_points: u16,
    pub open_groups: usize,
    pub settled_groups: usize,
    pub settleable_group_ids: Vec<ToolGroupId>,
    pub last_checkpoint: Option<TurnRecordId>,
    pub pressure: CheckpointPressure,
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCheckpointRequest {
    pub status: MemoryStatusSnapshot,
    pub tool_policy: CheckpointToolPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRolloutState {
    pub generation_id: CheckpointGenerationId,
    pub last_checkpoint: Option<TurnRecordId>,
    pub manifest_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactCaptureOutcome {
    Recorded(ToolCallRecord),
    Degraded(ArtifactCaptureFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCaptureFailure {
    pub call_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallRequest {
    pub artifact_id: ArtifactId,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallResult {
    pub artifact: ArtifactRef,
    pub data: Vec<u8>,
    pub truncated: bool,
}

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("checkpoint storage failed: {0}")]
    Storage(String),
    #[error("checkpoint state is degraded: {0}")]
    Degraded(String),
    #[error("invalid checkpoint request: {0}")]
    InvalidRequest(String),
    #[error("checkpoint evidence is missing or untrusted: {0}")]
    UntrustedEvidence(String),
    #[error("no pending checkpoint is available")]
    NoPendingCheckpoint,
}
