use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use codex_core::CodexThread;
use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_exec_server::ExecutorFileSystem;
use codex_extension_api::ExtensionDataInit;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentSessionMetadata;
use codex_project_agents::ProjectAgentTaskMetadata;
use codex_project_agents::ProjectAgentTaskPhase;
use codex_project_agents::ProjectAgentTaskResult;
use codex_project_agents::ProjectAgentTaskStatus;
use codex_project_agents::ProjectTaskId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadSource;
use codex_protocol::user_input::UserInput;
use tokio::sync::OwnedSemaphorePermit;

use crate::delegate::host_failed_result;
use crate::delegate::parse_worker_result;
use crate::delegate::prepare_worker_config;
use crate::delegate::project_agent_result_schema;
use crate::state::ProjectAgentRootContext;
use crate::state::ProjectAgentWorkerContext;

const WORKER_TURN_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 30 * 60);
const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 10);

pub(crate) struct WorkerThread {
    pub(crate) thread_id: ThreadId,
    pub(crate) thread: Arc<CodexThread>,
}

struct WorkerExecution {
    result: ProjectAgentTaskResult,
    phase: ProjectAgentTaskPhase,
}

enum WorkerTurnTerminal {
    Completed(String),
    Interrupted(String),
    SessionFailed(String),
}

pub(crate) async fn execute_queued_worker_task(
    thread_manager: Option<Arc<ThreadManager>>,
    root_context: Arc<ProjectAgentRootContext>,
    mut metadata: ProjectAgentTaskMetadata,
    semantic_task_id: Option<ProjectTaskId>,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
    _task_permit: OwnedSemaphorePermit,
) -> ProjectAgentTaskResult {
    let agent_id = metadata.agent_id.clone();
    let task_id = metadata.task_id.clone();

    let execution = match thread_manager {
        Some(thread_manager) => {
            run_worker(
                thread_manager,
                Arc::clone(&root_context),
                &mut metadata,
                semantic_task_id.as_ref(),
                file_system,
                scope,
            )
            .await
        }
        None => WorkerExecution {
            result: host_failed_result(
                &agent_id,
                &task_id,
                "project AGENT thread manager is unavailable",
            ),
            phase: ProjectAgentTaskPhase::Failed,
        },
    };
    let mut result = execution.result;
    if let Err(error) = persist_task_phase(
        root_context.as_ref(),
        file_system,
        scope,
        &mut metadata,
        execution.phase,
    )
    .await
    {
        result = host_failed_result(&agent_id, &task_id, &error);
        let _ = persist_task_phase(
            root_context.as_ref(),
            file_system,
            scope,
            &mut metadata,
            ProjectAgentTaskPhase::Failed,
        )
        .await;
    }
    persist_result(
        root_context.as_ref(),
        file_system,
        scope,
        &mut metadata,
        result,
    )
    .await
}

async fn run_worker(
    thread_manager: Arc<ThreadManager>,
    root_context: Arc<ProjectAgentRootContext>,
    metadata: &mut ProjectAgentTaskMetadata,
    semantic_task_id: Option<&ProjectTaskId>,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
) -> WorkerExecution {
    let agent_id = metadata.agent_id.clone();
    let task_id = metadata.task_id.clone();
    let worker = match load_or_start_worker(
        thread_manager.as_ref(),
        root_context.as_ref(),
        &agent_id,
        file_system,
        scope,
    )
    .await
    {
        Ok(worker) => worker,
        Err(error) => {
            return WorkerExecution {
                result: host_failed_result(&agent_id, &task_id, &error),
                phase: ProjectAgentTaskPhase::Failed,
            };
        }
    };
    metadata.session_thread_id = Some(worker.thread_id.to_string());
    if let Err(error) = persist_task_phase(
        root_context.as_ref(),
        file_system,
        scope,
        metadata,
        ProjectAgentTaskPhase::Running,
    )
    .await
    {
        return WorkerExecution {
            result: host_failed_result(&agent_id, &task_id, &error),
            phase: ProjectAgentTaskPhase::Failed,
        };
    }
    if let Some(semantic_task_id) = semantic_task_id
        && let Err(error) = crate::task_workspace::bind_worker_session(
            root_context.as_ref(),
            semantic_task_id,
            &task_id,
            worker.thread_id,
        )
        .await
    {
        return WorkerExecution {
            result: host_failed_result(&agent_id, &task_id, &error.to_string()),
            phase: ProjectAgentTaskPhase::Failed,
        };
    }

    let worker_result = tokio::time::timeout(WORKER_TURN_TIMEOUT, async {
        worker
            .thread
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: metadata.task.clone(),
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: Some(project_agent_result_schema(
                    &agent_id,
                    Some(&task_id),
                )),
                responsesapi_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .map_err(|error| format!("failed to submit project AGENT task: {error}"))?;
        loop {
            let event = worker
                .thread
                .next_event()
                .await
                .map_err(|error| format!("failed to read project AGENT event: {error}"))?;
            match event.msg {
                EventMsg::TurnComplete(completed) => {
                    let raw_result = completed.last_agent_message.ok_or_else(|| {
                        "project AGENT worker returned no final message".to_string()
                    })?;
                    return Ok(WorkerTurnTerminal::Completed(raw_result));
                }
                EventMsg::TurnAborted(aborted) => {
                    return Ok(WorkerTurnTerminal::Interrupted(format!(
                        "project AGENT turn was interrupted: {:?}",
                        aborted.reason
                    )));
                }
                EventMsg::ShutdownComplete => {
                    return Ok(WorkerTurnTerminal::SessionFailed(
                        "project AGENT worker shut down before completing the task".to_string(),
                    ));
                }
                _ => {}
            }
        }
    })
    .await;

    match worker_result {
        Ok(Ok(WorkerTurnTerminal::Completed(raw_result))) => {
            let result = parse_worker_result(&raw_result, &agent_id, &task_id);
            let phase = if result.status == ProjectAgentTaskStatus::Failed {
                ProjectAgentTaskPhase::Failed
            } else {
                ProjectAgentTaskPhase::Completed
            };
            WorkerExecution { result, phase }
        }
        Ok(Ok(WorkerTurnTerminal::Interrupted(error))) => WorkerExecution {
            result: host_failed_result(&agent_id, &task_id, &error),
            phase: ProjectAgentTaskPhase::Interrupted,
        },
        Ok(Ok(WorkerTurnTerminal::SessionFailed(error))) | Ok(Err(error)) => {
            discard_worker(
                thread_manager.as_ref(),
                root_context.as_ref(),
                &agent_id,
                &worker,
            )
            .await;
            WorkerExecution {
                result: host_failed_result(&agent_id, &task_id, &error),
                phase: ProjectAgentTaskPhase::Failed,
            }
        }
        Err(_) => {
            discard_worker(
                thread_manager.as_ref(),
                root_context.as_ref(),
                &agent_id,
                &worker,
            )
            .await;
            WorkerExecution {
                result: host_failed_result(
                    &agent_id,
                    &task_id,
                    "project AGENT worker exceeded the 30 minute turn limit",
                ),
                phase: ProjectAgentTaskPhase::Interrupted,
            }
        }
    }
}

pub(crate) async fn load_or_start_worker(
    thread_manager: &ThreadManager,
    root_context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
) -> Result<WorkerThread, String> {
    let stored_session = root_context
        .store
        .session_metadata(file_system, scope, agent_id)
        .await
        .map_err(|error| format!("failed to load project AGENT session metadata: {error}"))?;
    if let Some(thread_id) = root_context.active_session(agent_id).await {
        if let Some(worker) = reusable_worker(thread_manager, agent_id, thread_id).await {
            return Ok(worker);
        }
        root_context.forget_session(agent_id, thread_id).await;
    }
    if let Some(thread_id) = stored_session
        .as_ref()
        .and_then(|metadata| ThreadId::from_string(&metadata.thread_id).ok())
        && let Some(worker) = reusable_worker(thread_manager, agent_id, thread_id).await
    {
        root_context
            .remember_session(agent_id.clone(), thread_id)
            .await;
        return Ok(worker);
    }

    let runtime = root_context
        .store
        .load_runtime(file_system, scope, agent_id)
        .await
        .map_err(|error| format!("failed to load project AGENT runtime: {error}"))?;
    let config = prepare_worker_config(root_context.config.clone(), &runtime.entry.definition)?;
    let worker_context = ProjectAgentWorkerContext {
        thread_id: None,
        store: root_context.store.clone(),
        runtime,
        primary_environment_id: root_context.primary_environment_id.clone(),
    };
    let mut thread_extension_init = ExtensionDataInit::new();
    thread_extension_init.insert(worker_context);
    let new_thread = thread_manager
        .start_thread_with_options(StartThreadOptions {
            config,
            allow_provider_model_fallback: false,
            initial_history: InitialHistory::New,
            history_mode: None,
            session_source: Some(SessionSource::Custom(format!("project-agent:{agent_id}"))),
            thread_source: Some(ThreadSource::Feature("project-agent".to_string())),
            dynamic_tools: Vec::new(),
            metrics_service_name: None,
            parent_trace: None,
            environments: root_context.environments.clone(),
            thread_extension_init,
            supports_openai_form_elicitation: false,
        })
        .await
        .map_err(|error| format!("failed to start project AGENT worker: {error}"))?;
    let now = unix_timestamp();
    let session_metadata = ProjectAgentSessionMetadata {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agent_id: agent_id.clone(),
        thread_id: new_thread.thread_id.to_string(),
        generation: stored_session
            .as_ref()
            .map(|metadata| metadata.generation.saturating_add(/*rhs*/ 1))
            .unwrap_or(/*default*/ 1),
        created_at: now,
        updated_at: now,
    };
    if let Err(error) = root_context
        .store
        .persist_session_metadata(file_system, scope, &session_metadata)
        .await
    {
        let worker = WorkerThread {
            thread_id: new_thread.thread_id,
            thread: new_thread.thread,
        };
        discard_worker(thread_manager, root_context, agent_id, &worker).await;
        return Err(format!(
            "failed to persist project AGENT session metadata: {error}"
        ));
    }
    root_context
        .remember_session(agent_id.clone(), new_thread.thread_id)
        .await;
    Ok(WorkerThread {
        thread_id: new_thread.thread_id,
        thread: new_thread.thread,
    })
}

pub(crate) async fn reusable_worker(
    thread_manager: &ThreadManager,
    agent_id: &ProjectAgentId,
    thread_id: ThreadId,
) -> Option<WorkerThread> {
    let thread = thread_manager.get_thread(thread_id).await.ok()?;
    if matches!(
        thread.agent_status().await,
        AgentStatus::Shutdown | AgentStatus::NotFound
    ) {
        return None;
    }
    let context = thread.thread_extension::<ProjectAgentWorkerContext>()?;
    if context.runtime.entry.definition.id != *agent_id {
        return None;
    }
    Some(WorkerThread { thread_id, thread })
}

pub(crate) async fn discard_worker(
    thread_manager: &ThreadManager,
    root_context: &ProjectAgentRootContext,
    agent_id: &ProjectAgentId,
    worker: &WorkerThread,
) {
    let _ = tokio::time::timeout(WORKER_SHUTDOWN_TIMEOUT, worker.thread.shutdown_and_wait()).await;
    thread_manager.remove_thread(&worker.thread_id).await;
    root_context
        .forget_session(agent_id, worker.thread_id)
        .await;
}

pub(crate) async fn persist_task_phase(
    root_context: &ProjectAgentRootContext,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
    metadata: &mut ProjectAgentTaskMetadata,
    phase: ProjectAgentTaskPhase,
) -> Result<(), String> {
    metadata.phase = phase;
    metadata.updated_at = unix_timestamp().max(metadata.created_at);
    root_context
        .store
        .persist_task_metadata(file_system, scope, metadata)
        .await
        .map(|_| ())
        .map_err(|error| format!("failed to persist project AGENT task metadata: {error}"))
}

async fn persist_result(
    root_context: &ProjectAgentRootContext,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
    metadata: &mut ProjectAgentTaskMetadata,
    result: ProjectAgentTaskResult,
) -> ProjectAgentTaskResult {
    match root_context
        .store
        .persist_result(file_system, scope, &result)
        .await
    {
        Ok(_) => {
            root_context.emit_maintenance_status().await;
            result
        }
        Err(error) => {
            let _ = persist_task_phase(
                root_context,
                file_system,
                scope,
                metadata,
                ProjectAgentTaskPhase::Failed,
            )
            .await;
            host_failed_result(
                &result.agent_id,
                &result.task_id,
                &format!("failed to persist project AGENT result: {error}"),
            )
        }
    }
}

pub(crate) fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
