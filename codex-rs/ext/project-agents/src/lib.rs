//! Runtime integration for project-local specialist AGENT definitions.

pub use codex_project_agents::ProjectAgentId;
pub use codex_project_agents::ProjectAgentSessionMetadata;
pub use codex_project_agents::ProjectAgentStoreError;
pub use codex_project_agents::ProjectAgentTaskListLimit;
pub use codex_project_agents::ProjectAgentTaskMetadata;
pub use codex_project_agents::ProjectAgentTaskPhase as StoredProjectAgentTaskPhase;
pub use codex_project_agents::ProjectAgentTaskResult;
pub use codex_project_agents::ProjectAgentTaskStatus as StoredProjectAgentTaskStatus;

mod delegate;
mod events;
mod inspection;
mod maintenance;
mod state;
mod worker;
mod x_tools;

pub use inspection::ThreadProjectAgentDetail;
pub use inspection::ThreadProjectAgentListSnapshot;
pub use inspection::ThreadProjectAgentSummary;
pub use inspection::list_thread_project_agents;
pub use inspection::read_thread_project_agent;
pub use maintenance::ThreadProjectAgentMaintenanceOutcome;
pub use maintenance::maintain_thread_project_agents;
pub use state::install;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
