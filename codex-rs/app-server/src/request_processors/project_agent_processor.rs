use super::*;

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

    pub(crate) async fn maintenance_run(
        &self,
        request_id: ConnectionRequestId,
        params: ThreadProjectAgentMaintenanceRunParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let thread_id = ThreadId::from_string(&params.thread_id)
            .map_err(|error| invalid_request(format!("invalid thread id: {error}")))?;
        let thread = self
            .thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|_| invalid_request(format!("thread not found: {thread_id}")))?;
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
