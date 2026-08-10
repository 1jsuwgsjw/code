use codex_core::CodexThread;
use codex_project_agents::ProjectAgentMaintenanceError;
use codex_project_agents::ProjectAgentMaintenanceOptions;
use codex_project_agents::ProjectAgentMaintenanceOutcome;
use codex_project_agents::ProjectAgentMaintenanceTarget;

use crate::state::ProjectAgentRootContext;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentMaintenanceOutcome {
    pub project_root: String,
    pub outcome: ProjectAgentMaintenanceOutcome,
}

pub async fn maintain_thread_project_agents(
    thread: &CodexThread,
    actor: &str,
) -> Option<Result<ThreadProjectAgentMaintenanceOutcome, ProjectAgentMaintenanceError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(
        context
            .store
            .maintain(
                context.file_system.as_ref(),
                codex_project_agents::ProjectAgentFileSystemScope::Unrestricted,
                ProjectAgentMaintenanceTarget::All,
                ProjectAgentMaintenanceOptions::apply(actor),
            )
            .await
            .map(|outcome| ThreadProjectAgentMaintenanceOutcome {
                project_root: context.store.project_root().to_string(),
                outcome,
            }),
    )
}
