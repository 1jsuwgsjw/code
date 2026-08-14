//! Durable, model-directed context checkpoints for Codex sessions.
//!
//! This crate owns checkpoint identity, validation, storage, and state transitions. It deliberately
//! does not depend on Codex protocol or session types; hosts project their own conversation items
//! into the history ranges recorded here.

mod budget;
mod ids;
mod model;
mod projection;
mod reconcile;
mod runtime;
mod source_revision;
mod state;
mod store;

pub use budget::CheckpointBudget;
pub use ids::ArtifactId;
pub use ids::CheckpointGenerationId;
pub use ids::ToolGroupId;
pub use ids::TurnRecordId;
pub use model::ArtifactCaptureFailure;
pub use model::ArtifactCaptureOutcome;
pub use model::ArtifactRef;
pub use model::CheckpointError;
pub use model::CheckpointPressure;
pub use model::CheckpointRolloutState;
pub use model::CheckpointToolPolicy;
pub use model::ContextUsageSnapshot;
pub use model::EvidenceRef;
pub use model::InstalledCheckpoint;
pub use model::MemoryStatusSnapshot;
pub use model::PendingCheckpoint;
pub use model::PreparedCheckpointRequest;
pub use model::RecallRequest;
pub use model::RecallResult;
pub use model::ToolCallOutcome;
pub use model::ToolCallRecord;
pub use model::ToolGroupRecord;
pub use model::ToolGroupState;
pub use model::ToolInvocationRecord;
pub use model::ToolResultRecord;
pub use model::TruncationProvenance;
pub use model::TurnRecord;
pub use model::UpdateContextStateRequest;
pub use model::UsageSource;
pub use projection::project_history;
pub use runtime::CheckpointRuntime;
pub use state::ActiveContextState;
pub use state::ContextStateSnapshot;
pub use state::ContextStateUpdate;
pub use state::StateEntry;
pub use state::StateEntryKind;
pub use store::CheckpointStore;
pub use store::content_sha256;
