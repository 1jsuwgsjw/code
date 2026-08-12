//! Runtime integration for project-local specialist AGENT definitions.

pub use codex_project_agents::ProjectAgentId;
pub use codex_project_agents::ProjectAgentSessionMetadata;
pub use codex_project_agents::ProjectAgentStoreError;
pub use codex_project_agents::ProjectAgentTaskListLimit;
pub use codex_project_agents::ProjectAgentTaskMetadata;
pub use codex_project_agents::ProjectAgentTaskPhase as StoredProjectAgentTaskPhase;
pub use codex_project_agents::ProjectAgentTaskResult;
pub use codex_project_agents::ProjectAgentTaskStatus as StoredProjectAgentTaskStatus;
pub use codex_project_agents::ProjectTaskEvaluation;
pub use codex_project_agents::ProjectTaskEvaluationVerdict;
pub use codex_project_agents::ProjectTaskExecutor;
pub use codex_project_agents::ProjectTaskId;
pub use codex_project_agents::ProjectTaskNode;
pub use codex_project_agents::ProjectTaskResult;
pub use codex_project_agents::ProjectTaskResultStatus;
pub use codex_project_agents::ProjectTaskStatus;
pub use codex_project_agents::ProjectTaskSuggestedChild;
pub use codex_project_agents::ProjectTaskWorkspace;

mod control;
mod delegate;
mod events;
mod inspection;
mod maintenance;
mod state;
mod task_workspace;
mod worker;
mod x_tools;

pub use control::ProjectAgentControlError;
pub use control::ThreadProjectAgentRebuildOutcome;
pub use control::ThreadProjectAgentRetryOutcome;
pub use control::ThreadProjectAgentStartOutcome;
pub use control::ThreadProjectAgentTaskControl;
pub use control::follow_up_thread_project_agent;
pub use control::rebuild_thread_project_agent_session;
pub use control::retry_thread_project_agent;
pub use control::start_thread_project_agent_task;
pub use control::terminate_thread_project_agent;
pub use inspection::ThreadProjectAgentDetail;
pub use inspection::ThreadProjectAgentListSnapshot;
pub use inspection::ThreadProjectAgentSummary;
pub use inspection::list_thread_project_agents;
pub use inspection::read_thread_project_agent;
pub use maintenance::ThreadProjectAgentMaintenanceOutcome;
pub use maintenance::maintain_thread_project_agents;
pub use state::install;
pub use task_workspace::ThreadProjectTaskExecutionOutcome;
pub use task_workspace::append_thread_project_task_requirement;
pub use task_workspace::create_thread_project_child_task;
pub use task_workspace::create_thread_project_root_task;
pub use task_workspace::evaluate_thread_project_task;
pub use task_workspace::read_thread_project_task_workspace;
pub use task_workspace::record_thread_project_task_result;
pub use task_workspace::start_thread_project_task_execution;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
