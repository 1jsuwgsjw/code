use super::AppServerSession;
use codex_app_server_protocol as protocol;
use codex_app_server_protocol::ClientRequest;
use codex_protocol::ThreadId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;

impl AppServerSession {
    pub(crate) async fn thread_project_task_workspace_read(
        &mut self,
        thread_id: ThreadId,
    ) -> Result<protocol::ThreadProjectTaskWorkspaceReadResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectTaskWorkspaceRead {
                request_id,
                params: protocol::ThreadProjectTaskWorkspaceReadParams {
                    thread_id: thread_id.to_string(),
                },
            })
            .await
            .wrap_err("thread/projectTask/workspace/read failed in TUI")
    }

    pub(crate) async fn thread_project_task_create(
        &mut self,
        thread_id: ThreadId,
        parent_task_id: Option<String>,
        title: String,
        objective: String,
    ) -> Result<protocol::ThreadProjectTaskCreateResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectTaskCreate {
                request_id,
                params: protocol::ThreadProjectTaskCreateParams {
                    thread_id: thread_id.to_string(),
                    parent_task_id,
                    title,
                    objective,
                    executor: None,
                },
            })
            .await
            .wrap_err("thread/projectTask/create failed in TUI")
    }

    pub(crate) async fn thread_project_task_requirement_append(
        &mut self,
        thread_id: ThreadId,
        task_id: String,
        requirement: String,
    ) -> Result<protocol::ThreadProjectTaskRequirementAppendResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectTaskRequirementAppend {
                request_id,
                params: protocol::ThreadProjectTaskRequirementAppendParams {
                    thread_id: thread_id.to_string(),
                    task_id,
                    requirement,
                },
            })
            .await
            .wrap_err("thread/projectTask/requirement/append failed in TUI")
    }

    pub(crate) async fn thread_project_task_execution_start(
        &mut self,
        thread_id: ThreadId,
        task_id: String,
        agent_id: String,
    ) -> Result<protocol::ThreadProjectTaskExecutionStartResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectTaskExecutionStart {
                request_id,
                params: protocol::ThreadProjectTaskExecutionStartParams {
                    thread_id: thread_id.to_string(),
                    task_id,
                    agent_id,
                },
            })
            .await
            .wrap_err("thread/projectTask/execution/start failed in TUI")
    }

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

    pub(crate) async fn thread_project_agent_follow_up(
        &mut self,
        thread_id: ThreadId,
        agent_id: &str,
        session_thread_id: ThreadId,
        message: String,
    ) -> Result<protocol::ThreadProjectAgentFollowUpResponse> {
        let request_id = self.next_request_id();
        self.client
            .request_typed(ClientRequest::ThreadProjectAgentFollowUp {
                request_id,
                params: protocol::ThreadProjectAgentFollowUpParams {
                    thread_id: thread_id.to_string(),
                    agent_id: agent_id.to_string(),
                    session_thread_id: Some(session_thread_id.to_string()),
                    message,
                },
            })
            .await
            .wrap_err("thread/projectAgent/followUp failed in TUI")
    }
}
