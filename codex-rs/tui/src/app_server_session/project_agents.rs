use super::AppServerSession;
use codex_app_server_protocol as protocol;
use codex_app_server_protocol::ClientRequest;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;

impl AppServerSession {
    pub(crate) async fn thread_project_agent_list(
        &mut self,
        thread_id: ThreadId,
    ) -> Result<protocol::ThreadProjectAgentListResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentList {
                request_id,
                params: protocol::ThreadProjectAgentListParams {
                    thread_id: thread_id.to_string(),
                    cursor: None,
                    limit: Some(100),
                },
            })
            .await
            .wrap_err("thread/projectAgent/list failed in TUI")
    }

    pub(crate) async fn thread_project_agent_read(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
    ) -> Result<protocol::ThreadProjectAgentReadResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentRead {
                request_id,
                params: protocol::ThreadProjectAgentReadParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                    task_limit: Some(6),
                },
            })
            .await
            .wrap_err("thread/projectAgent/read failed in TUI")
    }

    pub(crate) async fn thread_project_agent_follow_up(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
        message: String,
    ) -> Result<protocol::ThreadProjectAgentFollowUpResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentFollowUp {
                request_id,
                params: protocol::ThreadProjectAgentFollowUpParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                    message,
                },
            })
            .await
            .wrap_err("thread/projectAgent/followUp failed in TUI")
    }

    pub(crate) async fn thread_project_agent_terminate(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
    ) -> Result<protocol::ThreadProjectAgentTerminateResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentTerminate {
                request_id,
                params: protocol::ThreadProjectAgentTerminateParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                },
            })
            .await
            .wrap_err("thread/projectAgent/terminate failed in TUI")
    }

    pub(crate) async fn thread_project_agent_retry(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
        task_id: &str,
    ) -> Result<protocol::ThreadProjectAgentRetryResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentRetry {
                request_id,
                params: protocol::ThreadProjectAgentRetryParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                    task_id: Some(task_id.to_string()),
                },
            })
            .await
            .wrap_err("thread/projectAgent/retry failed in TUI")
    }

    pub(crate) async fn thread_project_agent_rebuild(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
    ) -> Result<protocol::ThreadProjectAgentRebuildResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentRebuild {
                request_id,
                params: protocol::ThreadProjectAgentRebuildParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                },
            })
            .await
            .wrap_err("thread/projectAgent/rebuild failed in TUI")
    }
}
