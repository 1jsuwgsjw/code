//! Project-local, file-defined specialist AGENT primitives.

mod command_tool;
mod maintenance;
mod maintenance_store;
mod store;
mod task;
mod task_store;
mod task_workspace;
mod task_workspace_store;
mod types;

pub use maintenance::ProjectAgentMaintenanceAgentStatus;
pub use maintenance::ProjectAgentMaintenanceDecision;
pub use maintenance::ProjectAgentMaintenanceDisposition;
pub use maintenance::ProjectAgentMaintenanceItemKind;
pub use maintenance::ProjectAgentMaintenanceMode;
pub use maintenance::ProjectAgentMaintenanceOptions;
pub use maintenance::ProjectAgentMaintenanceOutcome;
pub use maintenance::ProjectAgentMaintenanceReport;
pub use maintenance::ProjectAgentMaintenanceStatus;
pub use maintenance::ProjectAgentMaintenanceTarget;
pub use maintenance::ProjectAgentPendingCounts;
pub use maintenance_store::ProjectAgentMaintenanceError;
pub use store::BootstrapDisposition;
pub use store::BootstrapOutcome;
pub use store::LoadedProjectAgentTool;
pub use store::ProjectAgentEntry;
pub use store::ProjectAgentFileSystemScope;
pub use store::ProjectAgentPersistenceOutcome;
pub use store::ProjectAgentRuntime;
pub use store::ProjectAgentStore;
pub use store::ProjectAgentStoreError;
pub use store::resolve_project_root;
pub use task::ProjectAgentSessionMetadata;
pub use task::ProjectAgentTaskListLimit;
pub use task::ProjectAgentTaskMetadata;
pub use task::ProjectAgentTaskPhase;
pub use task_workspace::ProjectTaskEvaluation;
pub use task_workspace::ProjectTaskEvaluationVerdict;
pub use task_workspace::ProjectTaskExecutor;
pub use task_workspace::ProjectTaskId;
pub use task_workspace::ProjectTaskNode;
pub use task_workspace::ProjectTaskResult;
pub use task_workspace::ProjectTaskResultStatus;
pub use task_workspace::ProjectTaskStatus;
pub use task_workspace::ProjectTaskSuggestedChild;
pub use task_workspace::ProjectTaskWorkspace;
pub use task_workspace::ProjectTaskWorkspaceError;
pub use types::PROJECT_AGENT_SCHEMA_VERSION;
pub use types::ProjectAgentDefinition;
pub use types::ProjectAgentId;
pub use types::ProjectAgentMemoryIndex;
pub use types::ProjectAgentRegistration;
pub use types::ProjectAgentRegistry;
pub use types::ProjectAgentTaskResult;
pub use types::ProjectAgentTaskStatus;
pub use types::ProjectAgentToolManifest;
pub use types::ProjectAgentToolTarget;
pub use types::ProjectAgentValidationError;
pub use types::RelativeProjectAgentPath;

#[cfg(test)]
#[path = "task_workspace_tests.rs"]
mod task_workspace_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
pub use command_tool::ProjectAgentCommandToolRegistration;
