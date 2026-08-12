use super::*;
use codex_app_server_protocol::ProjectAgentDefinition;
use codex_app_server_protocol::ProjectAgentPersistedResult;
use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_app_server_protocol::ProjectAgentSession;
use codex_app_server_protocol::ProjectAgentTask;
use codex_app_server_protocol::ProjectAgentTaskPhase;
use codex_app_server_protocol::ProjectAgentTaskStatus;
use codex_app_server_protocol::ThreadProjectAgentFollowUpParams;
use codex_app_server_protocol::ThreadProjectAgentFollowUpResponse;
use codex_app_server_protocol::ThreadProjectAgentListParams;
use codex_app_server_protocol::ThreadProjectAgentListResponse;
use codex_app_server_protocol::ThreadProjectAgentReadParams;
use codex_app_server_protocol::ThreadProjectAgentReadResponse;
use codex_app_server_protocol::ThreadProjectAgentRebuildParams;
use codex_app_server_protocol::ThreadProjectAgentRebuildResponse;
use codex_app_server_protocol::ThreadProjectAgentRetryParams;
use codex_app_server_protocol::ThreadProjectAgentRetryResponse;
use codex_app_server_protocol::ThreadProjectAgentStartParams;
use codex_app_server_protocol::ThreadProjectAgentStartResponse;
use codex_app_server_protocol::ThreadProjectAgentTerminateParams;
use codex_app_server_protocol::ThreadProjectAgentTerminateResponse;
use codex_project_agents_extension::ProjectAgentControlError;
use codex_project_agents_extension::ProjectAgentId;
use codex_project_agents_extension::ProjectAgentSessionMetadata;
use codex_project_agents_extension::ProjectAgentStoreError;
use codex_project_agents_extension::ProjectAgentTaskListLimit;
use codex_project_agents_extension::ProjectAgentTaskMetadata;
use codex_project_agents_extension::ProjectAgentTaskResult;
use codex_project_agents_extension::StoredProjectAgentTaskPhase;
use codex_project_agents_extension::StoredProjectAgentTaskStatus;
use codex_project_agents_extension::ThreadProjectAgentSummary;

const DEFAULT_PROJECT_AGENT_LIST_LIMIT: usize = 25;

#[derive(Clone)]
pub(crate) struct ProjectAgentRequestProcessor {
    thread_manager: Arc<ThreadManager>,
    outgoing: Arc<OutgoingMessageSender>,
}

impl ProjectAgentRequestProcessor {
    pub(crate) fn new(
        thread_manager: Arc<ThreadManager>,
        outgoing: Arc<OutgoingMessageSender>,
    ) -> Self {
        Self {
            thread_manager,
            outgoing,
        }
    }

    pub(crate) async fn list(
        &self,
        params: ThreadProjectAgentListParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.list_inner(params)
            .await
            .map(|response| Some(response.into()))
    }

    pub(crate) async fn read(
        &self,
        params: ThreadProjectAgentReadParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        self.read_inner(params)
            .await
            .map(|response| Some(response.into()))
    }

    async fn list_inner(
        &self,
        params: ThreadProjectAgentListParams,
    ) -> Result<ThreadProjectAgentListResponse, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let offset = parse_list_cursor(params.cursor.as_deref())?;
        let limit = params
            .limit
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_PROJECT_AGENT_LIST_LIMIT);
        let snapshot = codex_project_agents_extension::list_thread_project_agents(
            thread.as_ref(),
            offset,
            limit,
        )
        .await
        .ok_or_else(|| {
            invalid_request(format!(
                "thread has no project AGENT context: {}",
                params.thread_id
            ))
        })?
        .map_err(map_inspection_error)?;
        if offset > snapshot.total {
            return Err(invalid_request(format!(
                "cursor {offset} exceeds total project AGENTs {}",
                snapshot.total
            )));
        }

        let next_offset = offset.saturating_add(snapshot.agents.len());
        let next_cursor = (next_offset < snapshot.total).then_some(next_offset.to_string());
        Ok(ThreadProjectAgentListResponse {
            project_root: snapshot.project_root,
            data: snapshot.agents.iter().map(api_roster_entry).collect(),
            next_cursor,
            total: u64::try_from(snapshot.total).unwrap_or(u64::MAX),
        })
    }

    async fn read_inner(
        &self,
        params: ThreadProjectAgentReadParams,
    ) -> Result<ThreadProjectAgentReadResponse, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let history_limit = params
            .task_limit
            .and_then(|value| usize::try_from(value).ok())
            .map(ProjectAgentTaskListLimit::new)
            .unwrap_or_default();
        let detail = codex_project_agents_extension::read_thread_project_agent(
            thread.as_ref(),
            &agent_id,
            history_limit,
        )
        .await
        .ok_or_else(|| {
            invalid_request(format!(
                "thread has no project AGENT context: {}",
                params.thread_id
            ))
        })?
        .map_err(map_inspection_error)?;
        let summary = &detail.summary;

        Ok(ThreadProjectAgentReadResponse {
            project_root: detail.project_root,
            agent: api_agent_definition(summary),
            active_session_thread_id: summary.active_session_thread_id.clone(),
            session: summary.session.as_ref().map(api_session),
            current_task: summary.current_task.as_ref().map(api_task),
            current_result: detail.current_result.as_ref().map(api_result),
            recent_tasks: detail.recent_tasks.iter().map(api_task).collect(),
            recent_results: detail.recent_results.iter().map(api_result).collect(),
        })
    }

    pub(crate) async fn start(
        &self,
        params: ThreadProjectAgentStartParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let outcome = codex_project_agents_extension::start_thread_project_agent_task(
            Arc::clone(&self.thread_manager),
            thread.as_ref(),
            &agent_id,
            params.task,
        )
        .await
        .ok_or_else(|| missing_context(&params.thread_id))?
        .map_err(map_control_error)?;
        Ok(Some(
            ThreadProjectAgentStartResponse {
                task: api_task(&outcome.task),
            }
            .into(),
        ))
    }

    pub(crate) async fn follow_up(
        &self,
        params: ThreadProjectAgentFollowUpParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let outcome = codex_project_agents_extension::follow_up_thread_project_agent(
            self.thread_manager.as_ref(),
            thread.as_ref(),
            &agent_id,
            params.message,
        )
        .await
        .ok_or_else(|| missing_context(&params.thread_id))?
        .map_err(map_control_error)?;
        Ok(Some(
            ThreadProjectAgentFollowUpResponse {
                task_id: outcome.task_id,
                session_thread_id: outcome.session_thread_id,
            }
            .into(),
        ))
    }

    pub(crate) async fn terminate(
        &self,
        params: ThreadProjectAgentTerminateParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let outcome = codex_project_agents_extension::terminate_thread_project_agent(
            self.thread_manager.as_ref(),
            thread.as_ref(),
            &agent_id,
        )
        .await
        .ok_or_else(|| missing_context(&params.thread_id))?
        .map_err(map_control_error)?;
        Ok(Some(
            ThreadProjectAgentTerminateResponse {
                task_id: outcome.task_id,
                session_thread_id: outcome.session_thread_id,
            }
            .into(),
        ))
    }

    pub(crate) async fn retry(
        &self,
        params: ThreadProjectAgentRetryParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let outcome = codex_project_agents_extension::retry_thread_project_agent(
            Arc::clone(&self.thread_manager),
            thread.as_ref(),
            &agent_id,
            params.task_id.as_deref(),
        )
        .await
        .ok_or_else(|| missing_context(&params.thread_id))?
        .map_err(map_control_error)?;
        Ok(Some(
            ThreadProjectAgentRetryResponse {
                task: api_task(&outcome.task),
            }
            .into(),
        ))
    }

    pub(crate) async fn rebuild(
        &self,
        params: ThreadProjectAgentRebuildParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        let agent_id = parse_agent_id(params.agent_id)?;
        let outcome = codex_project_agents_extension::rebuild_thread_project_agent_session(
            self.thread_manager.as_ref(),
            thread.as_ref(),
            &agent_id,
        )
        .await
        .ok_or_else(|| missing_context(&params.thread_id))?
        .map_err(map_control_error)?;
        Ok(Some(
            ThreadProjectAgentRebuildResponse {
                previous_session_thread_id: outcome.previous_session_thread_id,
                session: api_session(&outcome.session),
            }
            .into(),
        ))
    }

    async fn loaded_thread(
        &self,
        thread_id: &str,
    ) -> Result<(ThreadId, Arc<CodexThread>), JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(thread_id)
            .map_err(|error| invalid_request(format!("invalid thread id: {error}")))?;
        let thread = self
            .thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|_| invalid_request(format!("thread not found: {thread_id}")))?;
        Ok((thread_id, thread))
    }

    pub(crate) async fn maintenance_run(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadProjectAgentMaintenanceRunParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (thread_id, thread) = self.loaded_thread(&params.thread_id).await?;
        if matches!(thread.agent_status().await, AgentStatus::Running) {
            return Err(invalid_request(
                "project AGENT maintenance is unavailable while a task is running",
            ));
        }

        let maintenance = codex_project_agents_extension::maintain_thread_project_agents(
            thread.as_ref(),
            "codex-app-server",
        )
        .await
        .ok_or_else(|| {
            invalid_request(format!("thread has no project AGENT context: {thread_id}"))
        })?
        .map_err(|error| internal_error(format!("project AGENT maintenance failed: {error}")))?;
        let accepted_count = maintenance
            .outcome
            .reports
            .iter()
            .fold(0u64, |total, report| {
                total.saturating_add(u64::try_from(report.accepted_count()).unwrap_or(u64::MAX))
            });
        let rejected_count = maintenance
            .outcome
            .reports
            .iter()
            .fold(0u64, |total, report| {
                total.saturating_add(u64::try_from(report.rejected_count()).unwrap_or(u64::MAX))
            });
        let status = ThreadProjectAgentMaintenanceStatusUpdatedNotification {
            thread_id: thread_id.to_string(),
            project_root: maintenance.project_root,
            pending_count: maintenance.outcome.status.pending().total(),
            catalog_revision: maintenance.outcome.status.catalog_revision,
        };
        self.outgoing
            .send_response(
                request_id,
                ThreadProjectAgentMaintenanceRunResponse {
                    accepted_count,
                    rejected_count,
                    status: status.clone(),
                },
            )
            .await;
        self.outgoing
            .send_server_notification(
                ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(status),
            )
            .await;
        Ok(None)
    }
}

fn parse_list_cursor(cursor: Option<&str>) -> Result<usize, JSONRPCErrorError> {
    cursor
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|error| invalid_request(format!("invalid project AGENT cursor: {error}")))
        .map(Option::unwrap_or_default)
}

fn parse_agent_id(agent_id: String) -> Result<ProjectAgentId, JSONRPCErrorError> {
    ProjectAgentId::new(agent_id)
        .map_err(|error| invalid_request(format!("invalid project AGENT id: {error}")))
}

fn missing_context(thread_id: &str) -> JSONRPCErrorError {
    invalid_request(format!("thread has no project AGENT context: {thread_id}"))
}

fn map_control_error(error: ProjectAgentControlError) -> JSONRPCErrorError {
    match error {
        ProjectAgentControlError::InvalidRequest(message) => invalid_request(message),
        ProjectAgentControlError::Internal(message) => internal_error(message),
        ProjectAgentControlError::Store(ProjectAgentStoreError::AgentNotFound(agent_id)) => {
            invalid_request(format!("project AGENT `{agent_id}` is not registered"))
        }
        ProjectAgentControlError::Store(error) => {
            internal_error(format!("project AGENT control failed: {error}"))
        }
    }
}

fn map_inspection_error(error: ProjectAgentStoreError) -> JSONRPCErrorError {
    match error {
        ProjectAgentStoreError::AgentNotFound(agent_id) => {
            invalid_request(format!("project AGENT `{agent_id}` is not registered"))
        }
        error => internal_error(format!("project AGENT inspection failed: {error}")),
    }
}

fn api_roster_entry(summary: &ThreadProjectAgentSummary) -> ProjectAgentRosterEntry {
    ProjectAgentRosterEntry {
        id: summary.entry.definition.id.to_string(),
        description: summary.entry.definition.description.clone(),
        enabled: summary.entry.enabled,
        active_session_thread_id: summary.active_session_thread_id.clone(),
        session: summary.session.as_ref().map(api_session),
        current_task: summary.current_task.as_ref().map(api_task),
    }
}

fn api_agent_definition(summary: &ThreadProjectAgentSummary) -> ProjectAgentDefinition {
    let definition = &summary.entry.definition;
    ProjectAgentDefinition {
        id: definition.id.to_string(),
        description: definition.description.clone(),
        enabled: summary.entry.enabled,
        definition_path: summary.entry.path.to_string(),
        constraints_file: definition.constraints_file.to_string(),
        model: definition.model.clone(),
        model_reasoning_effort: definition.model_reasoning_effort.clone(),
        model_provider: definition.model_provider.clone(),
        tools: definition.tools.iter().map(ToString::to_string).collect(),
        memory_max_items: definition.memory_max_items,
        memory_max_tokens: definition.memory_max_tokens,
    }
}

fn api_session(session: &ProjectAgentSessionMetadata) -> ProjectAgentSession {
    ProjectAgentSession {
        agent_id: session.agent_id.to_string(),
        thread_id: session.thread_id.clone(),
        generation: session.generation,
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}

fn api_task(task: &ProjectAgentTaskMetadata) -> ProjectAgentTask {
    ProjectAgentTask {
        agent_id: task.agent_id.to_string(),
        task_id: task.task_id.clone(),
        task: task.task.clone(),
        phase: match task.phase {
            StoredProjectAgentTaskPhase::Queued => ProjectAgentTaskPhase::Queued,
            StoredProjectAgentTaskPhase::Running => ProjectAgentTaskPhase::Running,
            StoredProjectAgentTaskPhase::Completed => ProjectAgentTaskPhase::Completed,
            StoredProjectAgentTaskPhase::Interrupted => ProjectAgentTaskPhase::Interrupted,
            StoredProjectAgentTaskPhase::Failed => ProjectAgentTaskPhase::Failed,
        },
        session_thread_id: task.session_thread_id.clone(),
        parent_thread_id: task.parent_thread_id.clone(),
        attempt: task.attempt,
        created_at: task.created_at,
        updated_at: task.updated_at,
    }
}

fn api_result(result: &ProjectAgentTaskResult) -> ProjectAgentPersistedResult {
    ProjectAgentPersistedResult {
        status: match result.status {
            StoredProjectAgentTaskStatus::Completed => ProjectAgentTaskStatus::Completed,
            StoredProjectAgentTaskStatus::RejectedOutOfScope => {
                ProjectAgentTaskStatus::RejectedOutOfScope
            }
            StoredProjectAgentTaskStatus::BlockedMissingTool => {
                ProjectAgentTaskStatus::BlockedMissingTool
            }
            StoredProjectAgentTaskStatus::Failed => ProjectAgentTaskStatus::Failed,
        },
        agent_id: result.agent_id.to_string(),
        task_id: result.task_id.clone(),
        result: result.result.clone(),
        artifacts: result.artifacts.clone(),
        evidence: result.evidence.clone(),
        memory_candidates: result.memory_candidates.clone(),
        improvement_proposals: result.improvement_proposals.clone(),
        error: result.error.clone(),
    }
}
