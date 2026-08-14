use std::time::Duration;

use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol as protocol;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ProjectTaskStatus;
use codex_app_server_protocol::RequestId;
use codex_protocol::ThreadId;

use super::App;
use crate::app_event::AppEvent;
use crate::app_server_session::AppServerSession;
use crate::project_agent_workbench::ProjectAgentMentionCatalog;
use crate::project_agent_workbench::ProjectAgentTaskMention;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;

const PROJECT_TASK_POLL_INTERVAL: Duration = Duration::from_millis(250);
const PROJECT_TASK_WAIT_TIMEOUT: Duration = Duration::from_secs(31 * 60);
const RECENT_PROJECT_AGENT_TASK_LIMIT: usize = 20;

impl App {
    pub(super) async fn refresh_project_agent_mentions(
        &mut self,
        app_server: &mut AppServerSession,
    ) {
        let Some(displayed_thread_id) = self.current_displayed_thread_id() else {
            self.chat_widget.on_project_agent_mentions_loaded(None);
            return;
        };
        let root_thread_id = self
            .project_agent_conversation_origin
            .as_ref()
            .filter(|origin| origin.worker_thread_id == displayed_thread_id)
            .map_or(displayed_thread_id, |origin| origin.root_thread_id);
        let agents = app_server
            .thread_project_agent_list(root_thread_id)
            .await
            .ok()
            .map(|response| response.data);
        let workspace = app_server
            .thread_project_task_workspace_read(root_thread_id)
            .await
            .ok()
            .map(|response| response.workspace);
        let project_agents = if agents.is_none() && workspace.is_none() {
            None
        } else {
            Some(project_agent_mention_catalog(
                root_thread_id,
                agents.unwrap_or_default(),
                workspace
                    .as_ref()
                    .map(|workspace| workspace.tasks.as_slice())
                    .unwrap_or_default(),
            ))
        };
        self.chat_widget
            .on_project_agent_mentions_loaded(project_agents);
    }

    pub(super) async fn handle_project_agent_workbench_action(
        &mut self,
        tui: &mut crate::tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        action: ProjectAgentWorkbenchAction,
    ) {
        match action {
            ProjectAgentWorkbenchAction::Open { selected_task_id } => {
                self.open_project_task_workspace(app_server, thread_id, selected_task_id)
                    .await;
            }
            ProjectAgentWorkbenchAction::PromptCreateRoot => {
                self.chat_widget
                    .show_project_task_create_prompt(thread_id, None);
            }
            ProjectAgentWorkbenchAction::PromptCreateChild(parent_task_id) => {
                self.chat_widget
                    .show_project_task_create_prompt(thread_id, Some(parent_task_id));
            }
            ProjectAgentWorkbenchAction::Create {
                parent_task_id,
                input,
            } => {
                let (title, objective) = split_task_input(&input);
                let selected_task_id = match app_server
                    .thread_project_task_create(thread_id, parent_task_id, title, objective)
                    .await
                {
                    Ok(response) => Some(response.task.task_id),
                    Err(err) => {
                        self.chat_widget
                            .add_error_message(format!("创建项目任务失败：{err}"));
                        return;
                    }
                };
                self.open_project_task_workspace(app_server, thread_id, selected_task_id)
                    .await;
            }
            ProjectAgentWorkbenchAction::PromptAppendRequirement(task_id) => {
                self.chat_widget
                    .show_project_task_requirement_prompt(thread_id, task_id);
            }
            ProjectAgentWorkbenchAction::AppendRequirement {
                task_id,
                requirement,
            } => {
                if let Err(err) = app_server
                    .thread_project_task_requirement_append(thread_id, task_id.clone(), requirement)
                    .await
                {
                    self.chat_widget
                        .add_error_message(format!("追加任务要求失败：{err}"));
                    return;
                }
                self.open_project_task_workspace(app_server, thread_id, Some(task_id))
                    .await;
            }
            ProjectAgentWorkbenchAction::PromptStartExecution(task_id) => {
                match app_server.thread_project_agent_list(thread_id).await {
                    Ok(response) => self.chat_widget.show_project_task_executor_picker(
                        thread_id,
                        task_id,
                        response.data,
                    ),
                    Err(err) => self
                        .chat_widget
                        .add_error_message(format!("项目 AGENT 名单读取失败：{err}")),
                }
            }
            ProjectAgentWorkbenchAction::StartExecution { task_id, agent_id } => {
                if let Err(err) = app_server
                    .thread_project_task_execution_start(thread_id, task_id.clone(), agent_id)
                    .await
                {
                    self.chat_widget
                        .add_error_message(format!("启动任务执行失败：{err}"));
                    return;
                }
                self.open_project_task_workspace(app_server, thread_id, Some(task_id.clone()))
                    .await;

                let request_handle = app_server.request_handle();
                let app_event_tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let result = wait_for_project_task(request_handle, thread_id, &task_id).await;
                    app_event_tx.send(AppEvent::ProjectTaskExecutionFinished {
                        thread_id,
                        task_id,
                        result,
                    });
                });
            }
            ProjectAgentWorkbenchAction::OpenConversation {
                task_id,
                task_title,
                agent_id,
                session_thread_id,
            } => {
                let session_thread_id = match ThreadId::from_string(&session_thread_id) {
                    Ok(thread_id) => thread_id,
                    Err(error) => {
                        self.chat_widget
                            .add_error_message(format!("项目 AGENT 会话 ID 无效：{error}"));
                        return;
                    }
                };
                if let Err(error) = self
                    .resume_project_agent_thread(tui, app_server, session_thread_id)
                    .await
                {
                    self.sync_active_agent_label();
                    self.chat_widget
                        .add_error_message(format!("打开项目 AGENT 会话失败：{error}"));
                    return;
                }
                self.project_agent_conversation_origin =
                    Some(super::ProjectAgentConversationOrigin {
                        root_thread_id: thread_id,
                        task_id,
                        worker_thread_id: session_thread_id,
                        active_agent_label: format!(
                            "@{agent_id} · {task_title} · Esc 返回任务树"
                        ),
                    });
                self.sync_active_agent_label();
            }
            ProjectAgentWorkbenchAction::Refresh(task_id) => {
                self.open_project_task_workspace(app_server, thread_id, Some(task_id))
                    .await;
            }
        }
    }

    pub(super) async fn return_from_project_agent_conversation(
        &mut self,
        tui: &mut crate::tui::Tui,
        app_server: &mut AppServerSession,
    ) -> bool {
        let Some(origin) = self.project_agent_conversation_origin.clone() else {
            return false;
        };
        if self.current_displayed_thread_id() != Some(origin.worker_thread_id) {
            self.sync_active_agent_label();
            return false;
        }
        if let Err(error) = self
            .resume_project_agent_thread(tui, app_server, origin.root_thread_id)
            .await
        {
            self.sync_active_agent_label();
            self.chat_widget
                .add_error_message(format!("返回项目任务树失败：{error}"));
            return true;
        }
        self.project_agent_conversation_origin = None;
        self.sync_active_agent_label();
        self.open_project_task_workspace(app_server, origin.root_thread_id, Some(origin.task_id))
            .await;
        true
    }

    async fn resume_project_agent_thread(
        &mut self,
        tui: &mut crate::tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> Result<(), String> {
        let resumed = app_server
            .resume_thread(self.config.clone(), thread_id)
            .await
            .map_err(|error| error.to_string())?;
        self.shutdown_current_thread(app_server).await;
        self.replace_chat_widget_with_app_server_thread(
            tui,
            app_server,
            resumed,
            /*initial_user_message*/ None,
        )
            .await
            .map_err(|error| error.to_string())
    }

    async fn open_project_task_workspace(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        selected_task_id: Option<String>,
    ) {
        match app_server
            .thread_project_task_workspace_read(thread_id)
            .await
        {
            Ok(response) => {
                let agents = app_server
                    .thread_project_agent_list(thread_id)
                    .await
                    .map(|response| response.data)
                    .unwrap_or_default();
                if self.current_displayed_thread_id() == Some(thread_id) {
                    let mention_catalog = project_agent_mention_catalog(
                        thread_id,
                        agents.clone(),
                        &response.workspace.tasks,
                    );
                    self.chat_widget
                        .on_project_agent_mentions_loaded(Some(mention_catalog));
                    self.chat_widget.show_project_task_workspace(
                        thread_id,
                        response.workspace,
                        agents,
                        selected_task_id,
                    );
                }
            }
            Err(err) => self
                .chat_widget
                .add_error_message(format!("项目任务工作区读取失败：{err}")),
        }
    }

    pub(super) fn handle_project_task_execution_finished(
        &mut self,
        thread_id: ThreadId,
        task_id: String,
        result: Result<protocol::ThreadProjectTaskWorkspaceReadResponse, String>,
    ) {
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }
        match result {
            Ok(response) => {
                let conversation = response
                    .workspace
                    .tasks
                    .iter()
                    .find(|task| task.task_id == task_id)
                    .and_then(|task| {
                        let codex_app_server_protocol::ProjectTaskExecutor::ProjectAgent {
                            agent_id,
                        } = &task.executor
                        else {
                            return None;
                        };
                        Some((
                            task.title.clone(),
                            agent_id.clone(),
                            task.session_thread_id.clone()?,
                        ))
                    });
                if let Some((task_title, agent_id, session_thread_id)) = conversation {
                    self.app_event_tx.send(AppEvent::ProjectAgentWorkbench {
                        thread_id,
                        action: ProjectAgentWorkbenchAction::OpenConversation {
                            task_id,
                            task_title,
                            agent_id,
                            session_thread_id,
                        },
                    });
                } else {
                    self.chat_widget.show_project_task_workspace(
                        thread_id,
                        response.workspace,
                        Vec::new(),
                        Some(task_id),
                    );
                }
            }
            Err(err) => self
                .chat_widget
                .add_error_message(format!("项目任务状态读取失败：{err}")),
        }
    }
}

fn project_agent_mention_catalog(
    root_thread_id: ThreadId,
    agents: Vec<protocol::ProjectAgentRosterEntry>,
    tasks: &[protocol::ProjectTask],
) -> ProjectAgentMentionCatalog {
    let mut recent_tasks = tasks
        .iter()
        .filter_map(|task| {
            let protocol::ProjectTaskExecutor::ProjectAgent { agent_id } = &task.executor else {
                return None;
            };
            Some((
                task.updated_at,
                ProjectAgentTaskMention {
                    root_thread_id,
                    task_id: task.task_id.clone(),
                    task_title: task.title.clone(),
                    agent_id: agent_id.clone(),
                    session_thread_id: task.session_thread_id.clone()?,
                },
            ))
        })
        .collect::<Vec<_>>();
    recent_tasks.sort_by(|(left, _), (right, _)| right.cmp(left));

    ProjectAgentMentionCatalog {
        agents,
        recent_tasks: recent_tasks
            .into_iter()
            .take(RECENT_PROJECT_AGENT_TASK_LIMIT)
            .map(|(_, task)| task)
            .collect(),
    }
}

fn split_task_input(input: &str) -> (String, String) {
    let input = input.trim();
    let mut lines = input.lines();
    let title = lines.next().unwrap_or_default().trim().to_string();
    let objective = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let objective = if objective.is_empty() {
        title.clone()
    } else {
        objective
    };
    (title, objective)
}

async fn wait_for_project_task(
    request_handle: AppServerRequestHandle,
    thread_id: ThreadId,
    task_id: &str,
) -> Result<protocol::ThreadProjectTaskWorkspaceReadResponse, String> {
    tokio::time::timeout(PROJECT_TASK_WAIT_TIMEOUT, async {
        let mut poll = 0_u64;
        loop {
            let response: protocol::ThreadProjectTaskWorkspaceReadResponse = request_handle
                .request_typed(ClientRequest::ThreadProjectTaskWorkspaceRead {
                    request_id: RequestId::String(format!("project-task-{task_id}-{poll}")),
                    params: protocol::ThreadProjectTaskWorkspaceReadParams {
                        thread_id: thread_id.to_string(),
                    },
                })
                .await
                .map_err(|err| err.to_string())?;
            let task = response
                .workspace
                .tasks
                .iter()
                .find(|task| task.task_id == task_id)
                .ok_or_else(|| format!("project task {task_id} disappeared"))?;
            if !matches!(
                task.status,
                ProjectTaskStatus::Pending | ProjectTaskStatus::InProgress
            ) {
                return Ok(response);
            }
            poll = poll.saturating_add(1);
            tokio::time::sleep(PROJECT_TASK_POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| format!("timed out waiting for project task {task_id}"))?
}
