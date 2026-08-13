use std::sync::Arc;

use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_exec_server::ExecutorFileSystem;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentTaskMetadata;
use codex_project_agents::ProjectAgentTaskPhase;
use codex_project_agents::ProjectAgentTaskResult;
use codex_project_agents::ProjectAgentTaskStatus;
use codex_project_agents::ProjectTaskEvaluation;
use codex_project_agents::ProjectTaskExecutor;
use codex_project_agents::ProjectTaskId;
use codex_project_agents::ProjectTaskNode;
use codex_project_agents::ProjectTaskResult;
use codex_project_agents::ProjectTaskResultStatus;
use codex_project_agents::ProjectTaskStatus;
use codex_project_agents::ProjectTaskSuggestedChild;
use codex_project_agents::ProjectTaskWorkspace;
use codex_project_agents::ProjectTaskWorkspaceError;
use codex_utils_string::take_bytes_at_char_boundary;
use uuid::Uuid;

use crate::control::ProjectAgentControlError;
use crate::control::ensure_enabled;
use crate::state::ProjectAgentRootContext;
use crate::worker::execute_queued_worker_task;
use crate::worker::persist_task_phase;
use crate::worker::unix_timestamp;

const MAX_RESULT_ITEMS: usize = 64;
const MAX_ITEM_BYTES: usize = 4 * 1024;
const MAX_SUGGESTIONS: usize = 16;
const MAX_OBJECTIVE_BYTES: usize = 8 * 1024;
const MAX_EXECUTION_TASK_BYTES: usize = 16 * 1024;

/// A semantic task after it has been bound to one internal project AGENT execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectTaskExecutionOutcome {
    pub task: ProjectTaskNode,
    pub execution_task_id: String,
}

pub async fn read_thread_project_task_workspace(
    thread: &CodexThread,
) -> Option<Result<ProjectTaskWorkspace, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(read_workspace(&context).await)
}

pub async fn create_thread_project_root_task(
    thread: &CodexThread,
    title: String,
    objective: String,
    executor: ProjectTaskExecutor,
) -> Option<Result<ProjectTaskNode, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(create_task(&context, None, title, objective, executor).await)
}

pub async fn create_thread_project_child_task(
    thread: &CodexThread,
    parent_task_id: ProjectTaskId,
    title: String,
    objective: String,
    executor: ProjectTaskExecutor,
) -> Option<Result<ProjectTaskNode, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(create_task(&context, Some(parent_task_id), title, objective, executor).await)
}

pub async fn append_thread_project_task_requirement(
    thread: &CodexThread,
    task_id: &ProjectTaskId,
    requirement: String,
) -> Option<Result<ProjectTaskNode, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(append_requirement(&context, task_id, requirement).await)
}

pub async fn start_thread_project_task_execution(
    thread_manager: Arc<ThreadManager>,
    thread: &CodexThread,
    task_id: &ProjectTaskId,
    agent_id: &ProjectAgentId,
) -> Option<Result<ThreadProjectTaskExecutionOutcome, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(start_execution(Some(thread_manager), context, task_id, agent_id).await)
}

pub async fn record_thread_project_task_result(
    thread: &CodexThread,
    task_id: &ProjectTaskId,
    result: ProjectTaskResult,
) -> Option<Result<ProjectTaskNode, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(record_result(&context, task_id, result).await)
}

pub async fn evaluate_thread_project_task(
    thread: &CodexThread,
    task_id: &ProjectTaskId,
    evaluation: ProjectTaskEvaluation,
) -> Option<Result<ProjectTaskNode, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(set_evaluation(&context, task_id, evaluation).await)
}

pub(crate) async fn execute_direct_delegate_task(
    thread_manager: Option<Arc<ThreadManager>>,
    context: Arc<ProjectAgentRootContext>,
    executor: ProjectTaskExecutor,
    agent_id: ProjectAgentId,
    objective: String,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
) -> ProjectAgentTaskResult {
    let semantic_task_id =
        ProjectTaskId::new(Uuid::now_v7().to_string()).expect("UUID project task IDs are valid");
    let semantic_task = match create_task_with_id(
        context.as_ref(),
        semantic_task_id.clone(),
        None,
        direct_delegate_title(&objective),
        objective,
        executor,
    )
    .await
    {
        Ok(task) => task,
        Err(error) => {
            return crate::delegate::host_failed_result(
                &agent_id,
                semantic_task_id.as_str(),
                &format!("failed to create the semantic project task: {error}"),
            );
        }
    };
    let created_at = unix_timestamp().max(semantic_task.updated_at);
    let mut execution = ProjectAgentTaskMetadata {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agent_id: agent_id.clone(),
        task_id: semantic_task.task_id.to_string(),
        task: task_prompt(&semantic_task),
        phase: ProjectAgentTaskPhase::Queued,
        session_thread_id: None,
        parent_thread_id: context.thread_id.to_string(),
        attempt: 1,
        created_at,
        updated_at: created_at,
    };
    let task_gate = context.task_gate(&agent_id).await;
    let task_permit = match task_gate.acquire_owned().await {
        Ok(permit) => permit,
        Err(error) => {
            return fail_direct_delegate_task(
                context.as_ref(),
                &semantic_task.task_id,
                &mut execution,
                file_system,
                scope,
                &format!("project AGENT task gate is unavailable: {error}"),
            )
            .await;
        }
    };
    let prepared: Result<(), ProjectAgentControlError> = async {
        let _permit = workspace_permit(&context).await?;
        let mut workspace = load_workspace(&context).await?;
        workspace
            .start_execution(
                &semantic_task.task_id,
                agent_id.clone(),
                semantic_task.task_id.to_string(),
                created_at,
            )
            .map_err(workspace_error)?;
        persist_workspace(&context, &workspace).await?;
        persist_task_phase(
            context.as_ref(),
            file_system,
            scope,
            &mut execution,
            ProjectAgentTaskPhase::Queued,
        )
        .await
        .map_err(ProjectAgentControlError::Internal)?;
        Ok(())
    }
    .await;
    if let Err(error) = prepared {
        return fail_direct_delegate_task(
            context.as_ref(),
            &semantic_task.task_id,
            &mut execution,
            file_system,
            scope,
            &error.to_string(),
        )
        .await;
    }
    let result = execute_queued_worker_task(
        thread_manager,
        Arc::clone(&context),
        execution,
        Some(semantic_task.task_id.clone()),
        file_system,
        scope,
        task_permit,
    )
    .await;
    if let Err(error) =
        write_back_worker_result(&context, &semantic_task.task_id, result.clone()).await
    {
        let failed =
            crate::delegate::host_failed_result(&agent_id, &result.task_id, &error.to_string());
        if let Err(retry_error) =
            write_back_worker_result(&context, &semantic_task.task_id, failed.clone()).await
        {
            tracing::error!(
                %retry_error,
                task_id = %semantic_task.task_id,
                execution_task_id = %result.task_id,
                "failed to terminate a semantic project AGENT task after result writeback failure"
            );
        }
        return failed;
    }
    result
}

async fn fail_direct_delegate_task(
    context: &ProjectAgentRootContext,
    semantic_task_id: &ProjectTaskId,
    execution: &mut ProjectAgentTaskMetadata,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
    error: &str,
) -> ProjectAgentTaskResult {
    let result =
        crate::delegate::host_failed_result(&execution.agent_id, &execution.task_id, error);
    if let Err(metadata_error) = persist_task_phase(
        context,
        file_system,
        scope,
        execution,
        ProjectAgentTaskPhase::Failed,
    )
    .await
    {
        tracing::error!(
            %metadata_error,
            task_id = %semantic_task_id,
            execution_task_id = %execution.task_id,
            "failed to persist terminal project AGENT execution metadata"
        );
    }
    if let Err(write_error) =
        write_back_worker_result(context, semantic_task_id, result.clone()).await
    {
        tracing::error!(
            %write_error,
            task_id = %semantic_task_id,
            execution_task_id = %execution.task_id,
            "failed to terminate a semantic project AGENT task after delegation failure"
        );
    }
    if let Err(result_error) = context
        .store
        .persist_result(file_system, scope, &result)
        .await
    {
        tracing::error!(
            %result_error,
            task_id = %semantic_task_id,
            execution_task_id = %execution.task_id,
            "failed to persist terminal project AGENT execution result"
        );
    } else {
        context.emit_maintenance_status().await;
    }
    result
}

async fn read_workspace(
    context: &ProjectAgentRootContext,
) -> Result<ProjectTaskWorkspace, ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    Ok(context
        .store
        .task_workspace(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
        )
        .await?)
}

async fn create_task(
    context: &ProjectAgentRootContext,
    parent_task_id: Option<ProjectTaskId>,
    title: String,
    objective: String,
    executor: ProjectTaskExecutor,
) -> Result<ProjectTaskNode, ProjectAgentControlError> {
    let task_id = ProjectTaskId::new(Uuid::now_v7().to_string()).map_err(workspace_error)?;
    create_task_with_id(context, task_id, parent_task_id, title, objective, executor).await
}

async fn create_task_with_id(
    context: &ProjectAgentRootContext,
    task_id: ProjectTaskId,
    parent_task_id: Option<ProjectTaskId>,
    title: String,
    objective: String,
    executor: ProjectTaskExecutor,
) -> Result<ProjectTaskNode, ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    workspace
        .create_task(
            task_id.clone(),
            parent_task_id,
            title,
            objective,
            executor,
            unix_timestamp(),
        )
        .map_err(workspace_error)?;
    let task = task(&workspace, &task_id)?.clone();
    persist_workspace(context, &workspace).await?;
    Ok(task)
}

async fn append_requirement(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
    requirement: String,
) -> Result<ProjectTaskNode, ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    let updated_at = task_update_timestamp(&workspace, task_id)?;
    workspace
        .append_requirement(task_id, requirement, updated_at)
        .map_err(workspace_error)?;
    let task = task(&workspace, task_id)?.clone();
    persist_workspace(context, &workspace).await?;
    Ok(task)
}

async fn start_execution(
    thread_manager: Option<Arc<ThreadManager>>,
    context: Arc<ProjectAgentRootContext>,
    task_id: &ProjectTaskId,
    agent_id: &ProjectAgentId,
) -> Result<ThreadProjectTaskExecutionOutcome, ProjectAgentControlError> {
    ensure_enabled(&context, agent_id).await?;
    let task_gate = context.task_gate(agent_id).await;
    let task_permit = task_gate.try_acquire_owned().map_err(|_| {
        ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT `{agent_id}` already has an active task"
        ))
    })?;
    let execution_task_id = Uuid::now_v7().to_string();
    let (task, execution) = {
        let _permit = workspace_permit(&context).await?;
        let mut workspace = load_workspace(&context).await?;
        let semantic_task = task(&workspace, task_id)?.clone();
        ensure_executor_matches(&semantic_task, agent_id)?;
        let created_at = unix_timestamp().max(semantic_task.updated_at);
        let mut execution = ProjectAgentTaskMetadata {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            agent_id: agent_id.clone(),
            task_id: execution_task_id.clone(),
            task: task_prompt(&semantic_task),
            phase: ProjectAgentTaskPhase::Queued,
            session_thread_id: None,
            parent_thread_id: context.thread_id.to_string(),
            attempt: 1,
            created_at,
            updated_at: created_at,
        };
        workspace
            .start_execution(task_id, agent_id.clone(), execution_task_id, created_at)
            .map_err(workspace_error)?;
        persist_task_phase(
            context.as_ref(),
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            &mut execution,
            ProjectAgentTaskPhase::Queued,
        )
        .await
        .map_err(ProjectAgentControlError::Internal)?;
        persist_workspace(&context, &workspace).await?;
        (task(&workspace, task_id)?.clone(), execution)
    };
    let outcome = ThreadProjectTaskExecutionOutcome {
        task,
        execution_task_id: execution.task_id.clone(),
    };
    let file_system = Arc::clone(&context.file_system);
    let semantic_task_id = task_id.clone();
    tokio::spawn(async move {
        let result = execute_queued_worker_task(
            thread_manager,
            Arc::clone(&context),
            execution,
            Some(semantic_task_id.clone()),
            file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            task_permit,
        )
        .await;
        if let Err(error) = write_back_worker_result(&context, &semantic_task_id, result).await {
            tracing::error!(%error, task_id = %semantic_task_id, "failed to write project AGENT result into the semantic task workspace");
        }
    });
    Ok(outcome)
}

pub(crate) async fn bind_worker_session(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
    execution_task_id: &str,
    session_thread_id: codex_protocol::ThreadId,
) -> Result<(), ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    let updated_at = task_update_timestamp(&workspace, task_id)?;
    workspace
        .bind_execution_session(
            task_id,
            execution_task_id,
            session_thread_id.to_string(),
            updated_at,
        )
        .map_err(workspace_error)?;
    persist_workspace(context, &workspace).await
}

async fn record_result(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
    result: ProjectTaskResult,
) -> Result<ProjectTaskNode, ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    workspace
        .record_result(task_id, result)
        .map_err(workspace_error)?;
    let task = task(&workspace, task_id)?.clone();
    persist_workspace(context, &workspace).await?;
    Ok(task)
}

async fn set_evaluation(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
    evaluation: ProjectTaskEvaluation,
) -> Result<ProjectTaskNode, ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    workspace
        .set_evaluation(task_id, evaluation)
        .map_err(workspace_error)?;
    let task = task(&workspace, task_id)?.clone();
    persist_workspace(context, &workspace).await?;
    Ok(task)
}

async fn write_back_worker_result(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
    result: ProjectAgentTaskResult,
) -> Result<(), ProjectAgentControlError> {
    let _permit = workspace_permit(context).await?;
    let mut workspace = load_workspace(context).await?;
    let completed_at = task_update_timestamp(&workspace, task_id)?;
    match task(&workspace, task_id)?.status {
        ProjectTaskStatus::Pending => workspace
            .start_execution(
                task_id,
                result.agent_id.clone(),
                result.task_id.clone(),
                completed_at,
            )
            .map_err(workspace_error)?,
        ProjectTaskStatus::InProgress => {
            let semantic_task = task(&workspace, task_id)?;
            ensure_executor_matches(semantic_task, &result.agent_id)?;
            let existing_execution_task_id = semantic_task.execution_task_id.clone();
            if existing_execution_task_id.as_deref() != Some(result.task_id.as_str()) {
                return Err(ProjectAgentControlError::InvalidRequest(format!(
                    "semantic task `{task_id}` is bound to execution `{:?}`, not `{}`",
                    existing_execution_task_id, result.task_id
                )));
            }
        }
        ProjectTaskStatus::Completed
        | ProjectTaskStatus::Rejected
        | ProjectTaskStatus::Blocked
        | ProjectTaskStatus::Failed => {
            return Ok(());
        }
    }
    workspace
        .record_result(task_id, worker_result(result, completed_at))
        .map_err(workspace_error)?;
    persist_workspace(context, &workspace).await
}

fn worker_result(result: ProjectAgentTaskResult, completed_at: i64) -> ProjectTaskResult {
    let status = match result.status {
        ProjectAgentTaskStatus::Completed => ProjectTaskResultStatus::Completed,
        ProjectAgentTaskStatus::RejectedOutOfScope => ProjectTaskResultStatus::Rejected,
        ProjectAgentTaskStatus::BlockedMissingTool => ProjectTaskResultStatus::BlockedMissingTool,
        ProjectAgentTaskStatus::Failed => ProjectTaskResultStatus::Failed,
    };
    let error = if status == ProjectTaskResultStatus::Failed {
        Some(bounded_item(result.error.as_deref().unwrap_or(
            "project AGENT execution failed without an error message",
        )))
    } else {
        None
    };
    ProjectTaskResult {
        status,
        summary: result.result,
        artifacts: bounded_items(result.artifacts),
        evidence: bounded_items(result.evidence),
        suggested_children: result
            .improvement_proposals
            .into_iter()
            .take(MAX_SUGGESTIONS)
            .enumerate()
            .map(|(index, objective)| ProjectTaskSuggestedChild {
                title: format!("Project AGENT follow-up {}", index + 1),
                objective: take_bytes_at_char_boundary(&objective, MAX_OBJECTIVE_BYTES).to_string(),
            })
            .collect(),
        error,
        completed_at,
    }
}

async fn load_workspace(
    context: &ProjectAgentRootContext,
) -> Result<ProjectTaskWorkspace, ProjectAgentControlError> {
    Ok(context
        .store
        .task_workspace(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
        )
        .await?)
}

async fn workspace_permit(
    context: &ProjectAgentRootContext,
) -> Result<tokio::sync::OwnedSemaphorePermit, ProjectAgentControlError> {
    Arc::clone(&context.task_workspace_gate)
        .acquire_owned()
        .await
        .map_err(|error| {
            ProjectAgentControlError::Internal(format!(
                "semantic task workspace gate is unavailable: {error}"
            ))
        })
}

async fn persist_workspace(
    context: &ProjectAgentRootContext,
    workspace: &ProjectTaskWorkspace,
) -> Result<(), ProjectAgentControlError> {
    context
        .store
        .persist_task_workspace(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            workspace,
        )
        .await?;
    Ok(())
}

fn task<'a>(
    workspace: &'a ProjectTaskWorkspace,
    task_id: &ProjectTaskId,
) -> Result<&'a ProjectTaskNode, ProjectAgentControlError> {
    workspace.task(task_id).ok_or_else(|| {
        ProjectAgentControlError::InvalidRequest(
            ProjectTaskWorkspaceError::TaskNotFound(task_id.clone()).to_string(),
        )
    })
}

fn task_update_timestamp(
    workspace: &ProjectTaskWorkspace,
    task_id: &ProjectTaskId,
) -> Result<i64, ProjectAgentControlError> {
    Ok(unix_timestamp().max(task(workspace, task_id)?.updated_at))
}

fn ensure_executor_matches(
    task: &ProjectTaskNode,
    agent_id: &ProjectAgentId,
) -> Result<(), ProjectAgentControlError> {
    if let ProjectTaskExecutor::ProjectAgent {
        agent_id: assigned_agent_id,
    } = &task.executor
        && assigned_agent_id != agent_id
    {
        return Err(ProjectAgentControlError::InvalidRequest(format!(
            "semantic task `{}` is assigned to project AGENT `{assigned_agent_id}`, not `{agent_id}`",
            task.task_id
        )));
    }
    Ok(())
}

fn task_prompt(task: &ProjectTaskNode) -> String {
    let mut prompt = format!("Task: {}\n\nObjective:\n{}", task.title, task.objective);
    if !task.requirements.is_empty() {
        prompt.push_str("\n\nAdditional requirements:\n");
        for requirement in &task.requirements {
            prompt.push_str("- ");
            prompt.push_str(requirement);
            prompt.push('\n');
        }
    }
    take_bytes_at_char_boundary(&prompt, MAX_EXECUTION_TASK_BYTES).to_string()
}

fn direct_delegate_title(objective: &str) -> String {
    let first_line = objective.lines().next().unwrap_or_default().trim();
    let title = take_bytes_at_char_boundary(first_line, 256).trim();
    if title.is_empty() {
        "Project AGENT task".to_string()
    } else {
        title.to_string()
    }
}

fn bounded_items(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .take(MAX_RESULT_ITEMS)
        .map(|value| bounded_item(&value))
        .collect()
}

fn bounded_item(value: &str) -> String {
    take_bytes_at_char_boundary(value, MAX_ITEM_BYTES).to_string()
}

fn workspace_error(error: impl ToString) -> ProjectAgentControlError {
    ProjectAgentControlError::InvalidRequest(error.to_string())
}

#[cfg(test)]
#[path = "task_workspace_tests.rs"]
mod tests;
