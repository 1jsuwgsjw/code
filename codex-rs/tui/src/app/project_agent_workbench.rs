use super::App;
use crate::app_server_session::AppServerSession;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;
use codex_protocol::ThreadId;

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
                    .map(|response| format!("已创建第 {} 次任务尝试。", response.task.attempt));
                (agent_id, result)
            }
            ProjectAgentWorkbenchAction::Rebuild(agent_id) => {
                let result = app_server
                    .thread_project_agent_rebuild(thread_id, &agent_id)
                    .await
                    .map(|response| format!("已重建为第 {} 代会话。", response.session.generation));
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
}
