use codex_core::CodexThread;
use codex_project_agents::ProjectAgentEntry;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentSessionMetadata;
use codex_project_agents::ProjectAgentStoreError;
use codex_project_agents::ProjectAgentTaskListLimit;
use codex_project_agents::ProjectAgentTaskMetadata;
use codex_project_agents::ProjectAgentTaskResult;

use crate::state::ProjectAgentRootContext;

const MAX_LIST_ITEMS: usize = 100;

/// One roster row assembled from the validated definition and durable runtime state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentSummary {
    pub entry: ProjectAgentEntry,
    pub active_session_thread_id: Option<String>,
    pub session: Option<ProjectAgentSessionMetadata>,
    pub current_task: Option<ProjectAgentTaskMetadata>,
}

/// One bounded roster page attached to a loaded root thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentListSnapshot {
    pub project_root: String,
    pub total: usize,
    pub agents: Vec<ThreadProjectAgentSummary>,
}

/// Complete durable state used by an AGENT detail view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentDetail {
    pub project_root: String,
    pub summary: ThreadProjectAgentSummary,
    pub current_result: Option<ProjectAgentTaskResult>,
    pub recent_tasks: Vec<ProjectAgentTaskMetadata>,
    pub recent_results: Vec<ProjectAgentTaskResult>,
}

pub async fn list_thread_project_agents(
    thread: &CodexThread,
    offset: usize,
    limit: usize,
) -> Option<Result<ThreadProjectAgentListSnapshot, ProjectAgentStoreError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(list_project_agents(&context, offset, limit).await)
}

pub async fn read_thread_project_agent(
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
    history_limit: ProjectAgentTaskListLimit,
) -> Option<Result<ThreadProjectAgentDetail, ProjectAgentStoreError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(read_project_agent(&context, agent_id, history_limit).await)
}

async fn list_project_agents(
    context: &ProjectAgentRootContext,
    offset: usize,
    limit: usize,
) -> Result<ThreadProjectAgentListSnapshot, ProjectAgentStoreError> {
    let entries = context
        .store
        .list(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
        )
        .await?;
    let total = entries.len();
    let limit = limit.clamp(1, MAX_LIST_ITEMS);
    let end = offset.saturating_add(limit).min(total);
    let page = if offset < total {
        &entries[offset..end]
    } else {
        &[]
    };
    let mut agents = Vec::with_capacity(page.len());
    for entry in page {
        agents.push(inspect_summary(context, entry.clone()).await?);
    }

    Ok(ThreadProjectAgentListSnapshot {
        project_root: context.store.project_root().to_string(),
        total,
        agents,
    })
}

async fn read_project_agent(
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    history_limit: ProjectAgentTaskListLimit,
) -> Result<ThreadProjectAgentDetail, ProjectAgentStoreError> {
    let entry = context
        .store
        .get(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;
    let summary = inspect_summary(context, entry).await?;
    let current_result = context
        .store
        .current_task_result(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;
    let recent_tasks = context
        .store
        .list_task_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
            history_limit,
        )
        .await?;
    let recent_results = context
        .store
        .list_task_results(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
            history_limit,
        )
        .await?;

    Ok(ThreadProjectAgentDetail {
        project_root: context.store.project_root().to_string(),
        summary,
        current_result,
        recent_tasks,
        recent_results,
    })
}

async fn inspect_summary(
    context: &ProjectAgentRootContext,
    entry: ProjectAgentEntry,
) -> Result<ThreadProjectAgentSummary, ProjectAgentStoreError> {
    let agent_id = &entry.definition.id;
    let active_session_thread_id = context
        .active_session(agent_id)
        .await
        .map(|thread_id| thread_id.to_string());
    let session = context
        .store
        .session_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;
    let current_task = context
        .store
        .current_task_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;

    Ok(ThreadProjectAgentSummary {
        entry,
        active_session_thread_id,
        session,
        current_task,
    })
}
