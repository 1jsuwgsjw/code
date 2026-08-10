//! Project-local, file-defined specialist AGENT primitives.

mod store;
mod types;

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
#[path = "tests.rs"]
mod tests;
