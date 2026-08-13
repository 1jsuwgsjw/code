use std::fmt;
use std::sync::Arc;

use codex_core::CodexThread;
use codex_core::ThreadManager;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentSessionMetadata;
use codex_project_agents::ProjectAgentStoreError;
use codex_project_agents::ProjectAgentTaskListLimit;
use codex_project_agents::ProjectAgentTaskMetadata;
use codex_project_agents::ProjectAgentTaskPhase;
use codex_protocol::ThreadId;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use uuid::Uuid;

use crate::delegate::MAX_TASK_BYTES;
use crate::state::ProjectAgentRootContext;
use crate::worker::WorkerThread;
use crate::worker::discard_worker;
use crate::worker::execute_queued_worker_task;
use crate::worker::load_or_start_worker;
use crate::worker::persist_task_phase;
use crate::worker::reusable_worker;
use crate::worker::unix_timestamp;

const MAX_FOLLOW_UP_BYTES: usize = 16 * 1024;

/// Active task identity returned after a follow-up or termination request is accepted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentTaskControl {
    pub task_id: String,
    pub session_thread_id: String,
}

/// A newly queued task for one named project AGENT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentStartOutcome {
    pub task: ProjectAgentTaskMetadata,
}

/// A newly queued retry that continues on the reusable worker session by default.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentRetryOutcome {
    pub task: ProjectAgentTaskMetadata,
}

/// The durable session rotation produced by an explicit rebuild request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadProjectAgentRebuildOutcome {
    pub previous_session_thread_id: Option<String>,
    pub session: ProjectAgentSessionMetadata,
}

/// Failure category used by app-server controls to preserve request versus host failures.
#[derive(Debug)]
pub enum ProjectAgentControlError {
    InvalidRequest(String),
    Internal(String),
    Store(ProjectAgentStoreError),
}

impl fmt::Display for ProjectAgentControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) | Self::Internal(message) => formatter.write_str(message),
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl From<ProjectAgentStoreError> for ProjectAgentControlError {
    fn from(error: ProjectAgentStoreError) -> Self {
        Self::Store(error)
    }
}

pub async fn follow_up_thread_project_agent(
    thread_manager: &ThreadManager,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
    message: String,
) -> Option<Result<ThreadProjectAgentTaskControl, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(follow_up(thread_manager, &context, agent_id, message).await)
}

pub async fn follow_up_thread_project_agent_session(
    thread_manager: &ThreadManager,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
    session_thread_id: ThreadId,
    message: String,
) -> Option<Result<ThreadProjectAgentTaskControl, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(
        follow_up_session(
            thread_manager,
            &context,
            agent_id,
            session_thread_id,
            message,
        )
        .await,
    )
}

pub async fn start_thread_project_agent_task(
    thread_manager: Arc<ThreadManager>,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
    task: String,
) -> Option<Result<ThreadProjectAgentStartOutcome, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(start(thread_manager, context, agent_id, task).await)
}

pub async fn terminate_thread_project_agent(
    thread_manager: &ThreadManager,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
) -> Option<Result<ThreadProjectAgentTaskControl, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(terminate(thread_manager, &context, agent_id).await)
}

pub async fn retry_thread_project_agent(
    thread_manager: Arc<ThreadManager>,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
    task_id: Option<&str>,
) -> Option<Result<ThreadProjectAgentRetryOutcome, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(retry(thread_manager, context, agent_id, task_id).await)
}

pub async fn rebuild_thread_project_agent_session(
    thread_manager: &ThreadManager,
    thread: &CodexThread,
    agent_id: &ProjectAgentId,
) -> Option<Result<ThreadProjectAgentRebuildOutcome, ProjectAgentControlError>> {
    let context = thread.thread_extension::<ProjectAgentRootContext>()?;
    Some(rebuild_session(thread_manager, &context, agent_id).await)
}

async fn follow_up(
    thread_manager: &ThreadManager,
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    message: String,
) -> Result<ThreadProjectAgentTaskControl, ProjectAgentControlError> {
    if message.trim().is_empty() || message.len() > MAX_FOLLOW_UP_BYTES {
        return Err(ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT follow-up must be non-empty and at most {MAX_FOLLOW_UP_BYTES} bytes"
        )));
    }
    let (task, worker) = active_worker(thread_manager, context, agent_id).await?;
    worker
        .thread
        .steer_input(
            vec![UserInput::Text {
                text: message,
                text_elements: Vec::new(),
            }],
            Default::default(),
            /*expected_turn_id*/ None,
            /*client_user_message_id*/ None,
            /*responsesapi_client_metadata*/ None,
        )
        .await
        .map_err(|error| {
            ProjectAgentControlError::InvalidRequest(format!(
                "project AGENT `{agent_id}` could not accept the follow-up: {error:?}"
            ))
        })?;
    Ok(task_control(&task, worker.thread_id))
}

async fn follow_up_session(
    thread_manager: &ThreadManager,
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    session_thread_id: ThreadId,
    message: String,
) -> Result<ThreadProjectAgentTaskControl, ProjectAgentControlError> {
    if message.trim().is_empty() || message.len() > MAX_FOLLOW_UP_BYTES {
        return Err(ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT follow-up must be non-empty and at most {MAX_FOLLOW_UP_BYTES} bytes"
        )));
    }
    ensure_enabled(context, agent_id).await?;
    let worker = reusable_worker(thread_manager, agent_id, session_thread_id)
        .await
        .ok_or_else(|| {
            ProjectAgentControlError::InvalidRequest(format!(
                "project AGENT `{agent_id}` session `{session_thread_id}` is not reusable"
            ))
        })?;
    if matches!(
        worker.thread.agent_status().await,
        codex_protocol::protocol::AgentStatus::Running
    ) {
        worker
            .thread
            .steer_input(
                vec![UserInput::Text {
                    text: message,
                    text_elements: Vec::new(),
                }],
                Default::default(),
                /*expected_turn_id*/ None,
                /*client_user_message_id*/ None,
                /*responsesapi_client_metadata*/ None,
            )
            .await
            .map_err(|error| {
                ProjectAgentControlError::InvalidRequest(format!(
                    "project AGENT `{agent_id}` could not accept the follow-up: {error:?}"
                ))
            })?;
    } else {
        worker
            .thread
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: message,
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: None,
                responsesapi_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .map_err(|error| {
                ProjectAgentControlError::InvalidRequest(format!(
                    "project AGENT `{agent_id}` could not start the follow-up: {error}"
                ))
            })?;
    }
    context
        .remember_session(agent_id.clone(), session_thread_id)
        .await;
    Ok(ThreadProjectAgentTaskControl {
        task_id: active_task_for_session(context, agent_id, session_thread_id)
            .await?
            .task_id,
        session_thread_id: session_thread_id.to_string(),
    })
}

async fn active_task_for_session(
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    session_thread_id: ThreadId,
) -> Result<ProjectAgentTaskMetadata, ProjectAgentControlError> {
    let session_thread_id = session_thread_id.to_string();
    context
        .store
        .list_task_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
            ProjectAgentTaskListLimit::new(usize::MAX),
        )
        .await?
        .into_iter()
        .filter(|task| task.session_thread_id.as_deref() == Some(session_thread_id.as_str()))
        .max_by_key(|task| task.updated_at)
        .ok_or_else(|| {
            ProjectAgentControlError::InvalidRequest(format!(
                "project AGENT `{agent_id}` session `{session_thread_id}` has no persisted task"
            ))
        })
}

async fn start(
    thread_manager: Arc<ThreadManager>,
    context: Arc<ProjectAgentRootContext>,
    agent_id: &ProjectAgentId,
    task: String,
) -> Result<ThreadProjectAgentStartOutcome, ProjectAgentControlError> {
    if task.trim().is_empty() || task.len() > MAX_TASK_BYTES {
        return Err(ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT task must be non-empty and at most {MAX_TASK_BYTES} bytes"
        )));
    }
    ensure_enabled(&context, agent_id).await?;
    let task_gate = context.task_gate(agent_id).await;
    let task_permit = task_gate.try_acquire_owned().map_err(|_| {
        ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT `{agent_id}` already has an active task"
        ))
    })?;
    let created_at = unix_timestamp();
    let mut metadata = ProjectAgentTaskMetadata {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agent_id: agent_id.clone(),
        task_id: Uuid::now_v7().to_string(),
        task,
        phase: ProjectAgentTaskPhase::Queued,
        session_thread_id: None,
        parent_thread_id: context.thread_id.to_string(),
        attempt: 1,
        created_at,
        updated_at: created_at,
    };
    persist_task_phase(
        context.as_ref(),
        context.file_system.as_ref(),
        ProjectAgentFileSystemScope::Unrestricted,
        &mut metadata,
        ProjectAgentTaskPhase::Queued,
    )
    .await
    .map_err(ProjectAgentControlError::Internal)?;
    let outcome = ThreadProjectAgentStartOutcome {
        task: metadata.clone(),
    };
    let file_system = Arc::clone(&context.file_system);
    tokio::spawn(async move {
        let _ = execute_queued_worker_task(
            Some(thread_manager),
            context,
            metadata,
            /*semantic_task_id*/ None,
            file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            task_permit,
        )
        .await;
    });
    Ok(outcome)
}

async fn terminate(
    thread_manager: &ThreadManager,
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
) -> Result<ThreadProjectAgentTaskControl, ProjectAgentControlError> {
    let (task, worker) = active_worker(thread_manager, context, agent_id).await?;
    worker.thread.submit(Op::Interrupt).await.map_err(|error| {
        ProjectAgentControlError::Internal(format!(
            "failed to interrupt project AGENT `{agent_id}`: {error}"
        ))
    })?;
    Ok(task_control(&task, worker.thread_id))
}

async fn retry(
    thread_manager: Arc<ThreadManager>,
    context: Arc<ProjectAgentRootContext>,
    agent_id: &ProjectAgentId,
    task_id: Option<&str>,
) -> Result<ThreadProjectAgentRetryOutcome, ProjectAgentControlError> {
    ensure_enabled(&context, agent_id).await?;
    let task_gate = context.task_gate(agent_id).await;
    let task_permit = task_gate.try_acquire_owned().map_err(|_| {
        ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT `{agent_id}` already has an active task"
        ))
    })?;
    let source = match task_id {
        Some(task_id) => {
            context
                .store
                .task_metadata(
                    context.file_system.as_ref(),
                    ProjectAgentFileSystemScope::Unrestricted,
                    agent_id,
                    task_id,
                )
                .await?
        }
        None => {
            context
                .store
                .current_task_metadata(
                    context.file_system.as_ref(),
                    ProjectAgentFileSystemScope::Unrestricted,
                    agent_id,
                )
                .await?
        }
    }
    .ok_or_else(|| {
        ProjectAgentControlError::InvalidRequest(match task_id {
            Some(task_id) => format!("project AGENT task not found: {agent_id}/{task_id}"),
            None => format!("project AGENT `{agent_id}` has no task to retry"),
        })
    })?;
    if source.phase.is_active() {
        return Err(ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT task `{}` is still active",
            source.task_id
        )));
    }

    let created_at = unix_timestamp();
    let mut task = ProjectAgentTaskMetadata {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agent_id: agent_id.clone(),
        task_id: Uuid::now_v7().to_string(),
        task: source.task,
        phase: ProjectAgentTaskPhase::Queued,
        session_thread_id: None,
        parent_thread_id: context.thread_id.to_string(),
        attempt: source.attempt.saturating_add(/*rhs*/ 1),
        created_at,
        updated_at: created_at,
    };
    persist_task_phase(
        context.as_ref(),
        context.file_system.as_ref(),
        ProjectAgentFileSystemScope::Unrestricted,
        &mut task,
        ProjectAgentTaskPhase::Queued,
    )
    .await
    .map_err(ProjectAgentControlError::Internal)?;
    let outcome = ThreadProjectAgentRetryOutcome { task: task.clone() };
    let file_system = Arc::clone(&context.file_system);
    tokio::spawn(async move {
        let _ = execute_queued_worker_task(
            Some(thread_manager),
            context,
            task,
            /*semantic_task_id*/ None,
            file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            task_permit,
        )
        .await;
    });
    Ok(outcome)
}

async fn rebuild_session(
    thread_manager: &ThreadManager,
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
) -> Result<ThreadProjectAgentRebuildOutcome, ProjectAgentControlError> {
    ensure_enabled(context, agent_id).await?;
    let task_gate = context.task_gate(agent_id).await;
    let _task_permit = task_gate.try_acquire_owned().map_err(|_| {
        ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT `{agent_id}` session cannot be rebuilt while a task is active"
        ))
    })?;
    let stored_session = context
        .store
        .session_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;
    let active_session = context.active_session(agent_id).await;
    let previous_session_thread_id = active_session
        .map(|thread_id| thread_id.to_string())
        .or_else(|| {
            stored_session
                .as_ref()
                .map(|session| session.thread_id.clone())
        });
    let mut candidates = Vec::new();
    if let Some(thread_id) = active_session {
        candidates.push(thread_id);
    }
    if let Some(thread_id) = stored_session
        .as_ref()
        .and_then(|session| ThreadId::from_string(&session.thread_id).ok())
        && !candidates.contains(&thread_id)
    {
        candidates.push(thread_id);
    }
    for thread_id in candidates {
        if let Some(worker) = reusable_worker(thread_manager, agent_id, thread_id).await {
            discard_worker(thread_manager, context, agent_id, &worker).await;
        } else {
            context.forget_session(agent_id, thread_id).await;
        }
    }

    load_or_start_worker(
        thread_manager,
        context,
        agent_id,
        context.file_system.as_ref(),
        ProjectAgentFileSystemScope::Unrestricted,
    )
    .await
    .map_err(ProjectAgentControlError::Internal)?;
    let session = context
        .store
        .session_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?
        .ok_or_else(|| {
            ProjectAgentControlError::Internal(format!(
                "project AGENT `{agent_id}` rebuilt without persisted session metadata"
            ))
        })?;
    Ok(ThreadProjectAgentRebuildOutcome {
        previous_session_thread_id,
        session,
    })
}

async fn active_worker(
    thread_manager: &ThreadManager,
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
) -> Result<(ProjectAgentTaskMetadata, WorkerThread), ProjectAgentControlError> {
    let task = context
        .store
        .current_task_metadata(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?
        .filter(|task| task.phase.is_active())
        .ok_or_else(|| {
            ProjectAgentControlError::InvalidRequest(format!(
                "project AGENT `{agent_id}` has no active task"
            ))
        })?;
    let mut candidates = Vec::new();
    if let Some(thread_id) = task
        .session_thread_id
        .as_deref()
        .and_then(|thread_id| ThreadId::from_string(thread_id).ok())
    {
        candidates.push(thread_id);
    }
    if let Some(thread_id) = context.active_session(agent_id).await
        && !candidates.contains(&thread_id)
    {
        candidates.push(thread_id);
    }
    for thread_id in candidates {
        if let Some(worker) = reusable_worker(thread_manager, agent_id, thread_id).await {
            context.remember_session(agent_id.clone(), thread_id).await;
            return Ok((task, worker));
        }
    }
    Err(ProjectAgentControlError::InvalidRequest(format!(
        "project AGENT `{agent_id}` has no reusable worker session for its active task"
    )))
}

pub(crate) async fn ensure_enabled(
    context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
) -> Result<(), ProjectAgentControlError> {
    let entry = context
        .store
        .get(
            context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id,
        )
        .await?;
    if entry.enabled {
        Ok(())
    } else {
        Err(ProjectAgentControlError::InvalidRequest(format!(
            "project AGENT `{agent_id}` is disabled"
        )))
    }
}

fn task_control(
    task: &ProjectAgentTaskMetadata,
    session_thread_id: ThreadId,
) -> ThreadProjectAgentTaskControl {
    ThreadProjectAgentTaskControl {
        task_id: task.task_id.clone(),
        session_thread_id: session_thread_id.to_string(),
    }
}
