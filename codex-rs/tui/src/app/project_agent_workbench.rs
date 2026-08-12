use super::App;
use crate::app_server_session::AppServerSession;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol as protocol;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectAgentTaskPhase;
use codex_app_server_protocol::RequestId;
use codex_protocol::ThreadId;
use std::time::Duration;

const PROJECT_AGENT_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PROJECT_AGENT_RESULT_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const PROJECT_AGENT_TASK_WAIT_TIMEOUT: Duration = Duration::from_secs(31 * 60);

impl App {
    pub(super) async fn handle_project_agent_workbench_action(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        action: ProjectAgentWorkbenchAction,
    ) {
        let (agent_id, result) = match action {
            ProjectAgentWorkbenchAction::List => {
                let result = app_server.thread_project_agent_list(thread_id).await;
                if self.current_displayed_thread_id() != Some(thread_id) {
                    return;
                }
                match result {
                    Ok(response) => self
                        .chat_widget
                        .show_project_agent_roster(thread_id, response),
                    Err(err) => self
                        .chat_widget
                        .add_error_message(format!("加载项目 AGENT 列表失败：{err}")),
                }
                return;
            }
            ProjectAgentWorkbenchAction::Read(agent_id) => {
                let result = app_server
                    .thread_project_agent_read(thread_id, &agent_id)
                    .await;
                if self.current_displayed_thread_id() != Some(thread_id) {
                    return;
                }
                match result {
                    Ok(response) => self
                        .chat_widget
                        .show_project_agent_detail(thread_id, response),
                    Err(err) => self
                        .chat_widget
                        .add_error_message(format!("读取项目 AGENT @{agent_id} 失败：{err}")),
                }
                return;
            }
            ProjectAgentWorkbenchAction::PromptStart(agent_id) => {
                if self.current_displayed_thread_id() == Some(thread_id) {
                    self.chat_widget
                        .show_project_agent_start_prompt(thread_id, agent_id);
                }
                return;
            }
            ProjectAgentWorkbenchAction::Start { agent_id, task } => {
                let result = app_server
                    .thread_project_agent_start(thread_id, &agent_id, task)
                    .await;
                if self.current_displayed_thread_id() != Some(thread_id) {
                    return;
                }
                match result {
                    Ok(response) => {
                        let task_id = response.task.task_id.clone();
                        self.chat_widget
                            .show_project_agent_task_started(&response.task);
                        let request_handle = app_server.request_handle();
                        let app_event_tx = self.app_event_tx.clone();
                        tokio::spawn(async move {
                            let result = wait_for_project_agent_task(
                                request_handle,
                                thread_id,
                                &agent_id,
                                &task_id,
                            )
                            .await;
                            app_event_tx.send(AppEvent::ProjectAgentTaskFinished {
                                thread_id,
                                agent_id,
                                task_id,
                                result,
                            });
                        });
                    }
                    Err(err) => self
                        .chat_widget
                        .add_error_message(format!("项目 AGENT @{agent_id} 发起任务失败：{err}")),
                }
                return;
            }
            ProjectAgentWorkbenchAction::PromptFollowUp(agent_id) => {
                if self.current_displayed_thread_id() == Some(thread_id) {
                    self.chat_widget
                        .show_project_agent_follow_up_prompt(thread_id, agent_id);
                }
                return;
            }
            ProjectAgentWorkbenchAction::FollowUp { agent_id, message } => {
                let result = app_server
                    .thread_project_agent_follow_up(thread_id, &agent_id, message)
                    .await
                    .map(|_| "已追加跟进。".to_string());
                (agent_id, result)
            }
            ProjectAgentWorkbenchAction::Terminate(agent_id) => {
                let result = app_server
                    .thread_project_agent_terminate(thread_id, &agent_id)
                    .await
                    .map(|_| "已请求终止当前任务。".to_string());
                (agent_id, result)
            }
            ProjectAgentWorkbenchAction::Retry { agent_id, task_id } => {
                let result = app_server
                    .thread_project_agent_retry(thread_id, &agent_id, &task_id)
                    .await
                    .map(|response| {
                        let attempt = response.task.attempt;
                        format!("已创建第 {attempt} 次任务尝试。")
                    });
                (agent_id, result)
            }
            ProjectAgentWorkbenchAction::Rebuild(agent_id) => {
                let result = app_server
                    .thread_project_agent_rebuild(thread_id, &agent_id)
                    .await
                    .map(|response| {
                        let generation = response.session.generation;
                        format!("已重建为第 {generation} 代会话。")
                    });
                (agent_id, result)
            }
        };
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }
        match result {
            Ok(message) => self.chat_widget.add_info_message(
                format!("项目 AGENT @{agent_id}：{message}"),
                /*hint*/ None,
            ),
            Err(err) => self
                .chat_widget
                .add_error_message(format!("项目 AGENT @{agent_id} 操作失败：{err}")),
        }
    }

    pub(super) fn handle_project_agent_task_finished(
        &mut self,
        thread_id: ThreadId,
        agent_id: String,
        task_id: String,
        result: Result<protocol::ThreadProjectAgentReadResponse, String>,
    ) {
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }
        match result {
            Ok(response) => self
                .chat_widget
                .show_project_agent_task_finished(&agent_id, &task_id, response),
            Err(err) => self.chat_widget.add_error_message(format!(
                "项目 AGENT @{agent_id} 任务 {task_id} 状态读取失败：{err}"
            )),
        }
    }
}

async fn wait_for_project_agent_task(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
    agent_id: &str,
    task_id: &str,
) -> Result<protocol::ThreadProjectAgentReadResponse, String> {
    tokio::time::timeout(PROJECT_AGENT_TASK_WAIT_TIMEOUT, async {
        let mut poll = 0_u64;
        let mut terminal_task_observed_at = None;
        loop {
            let response: protocol::ThreadProjectAgentReadResponse = request_handle
                .request_typed(ClientRequest::ThreadProjectAgentRead {
                    request_id: RequestId::String(format!("project-agent-task-{task_id}-{poll}")),
                    params: protocol::ThreadProjectAgentReadParams {
                        thread_id: thread_id.to_string(),
                        agent_id: agent_id.to_string(),
                        task_limit: Some(6),
                    },
                })
                .await
                .map_err(|err| err.to_string())?;
            let task = response
                .current_task
                .as_ref()
                .filter(|task| task.task_id == task_id)
                .or_else(|| {
                    response
                        .recent_tasks
                        .iter()
                        .find(|task| task.task_id == task_id)
                });
            let task_is_terminal = task.is_some_and(|task| {
                !matches!(
                    task.phase,
                    ProjectAgentTaskPhase::Queued | ProjectAgentTaskPhase::Running
                )
            });
            let has_persisted_result = response
                .current_result
                .as_ref()
                .is_some_and(|result| result.task_id == task_id)
                || response
                    .recent_results
                    .iter()
                    .any(|result| result.task_id == task_id);
            if task_is_terminal && has_persisted_result {
                return Ok(response);
            }
            if task_is_terminal {
                let observed_at =
                    terminal_task_observed_at.get_or_insert_with(tokio::time::Instant::now);
                if observed_at.elapsed() >= PROJECT_AGENT_RESULT_WAIT_TIMEOUT {
                    return Err(format!(
                        "project AGENT task {task_id} ended without a persisted result"
                    ));
                }
            } else {
                terminal_task_observed_at = None;
            }
            poll = poll.saturating_add(1);
            tokio::time::sleep(PROJECT_AGENT_POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for project AGENT task {task_id}"))?
}
