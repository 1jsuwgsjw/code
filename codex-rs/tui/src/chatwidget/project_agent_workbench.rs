use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_app_server_protocol::ProjectTask;
use codex_app_server_protocol::ProjectTaskEvaluationVerdict;
use codex_app_server_protocol::ProjectTaskExecutor;
use codex_app_server_protocol::ProjectTaskResultStatus;
use codex_app_server_protocol::ProjectTaskStatus;
use codex_app_server_protocol::ProjectTaskWorkspace;
use codex_protocol::ThreadId;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::ColumnWidthMode;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::SideContentWidth;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::text_formatting::truncate_text;

const PROJECT_TASK_WORKBENCH_VIEW_ID: &str = "project-task-workbench";
const PROJECT_TASK_USAGE: &str = "用法：/agent 或 /agent <task-id>";

impl ChatWidget {
    pub(super) fn dispatch_project_agent_workbench(&mut self, args: &str) {
        let Some(thread_id) = self.thread_id else {
            self.add_error_message("会话启动后才能打开项目任务工作区。".to_string());
            return;
        };
        let selected_task_id = match args.trim() {
            "" => None,
            value if !value.contains(char::is_whitespace) => Some(value.to_string()),
            _ => {
                self.add_error_message(PROJECT_TASK_USAGE.to_string());
                return;
            }
        };
        self.app_event_tx.send(AppEvent::ProjectAgentWorkbench {
            thread_id,
            action: ProjectAgentWorkbenchAction::Open { selected_task_id },
        });
    }

    pub(crate) fn show_project_task_workspace(
        &mut self,
        thread_id: ThreadId,
        workspace: ProjectTaskWorkspace,
        agents: Vec<ProjectAgentRosterEntry>,
        selected_task_id: Option<String>,
    ) {
        let tasks = Arc::new(workspace.tasks);
        let agents = Arc::new(agents);
        let selected_task_index = selected_task_id
            .as_deref()
            .and_then(|task_id| tasks.iter().position(|task| task.task_id == task_id))
            .unwrap_or(0);
        let selected_task_index = Arc::new(AtomicUsize::new(selected_task_index));
        let mut header = ColumnRenderable::new();
        header.push(Line::from("项目任务工作区".cyan().bold()));
        header.push(Line::from("任务是一级对象；执行者只负责当前节点。".dim()));

        let mut items = Vec::new();
        let mut item_task_indices = Vec::new();
        items.push(action_item(
            thread_id,
            "＋ 新建根任务",
            "建立新的任务目标。",
            ProjectAgentWorkbenchAction::PromptCreateRoot,
        ));
        item_task_indices.push(None);
        let mut initial_selected_idx = None;
        for (task, tree_prefix) in task_tree(&tasks) {
            let task_index = tasks
                .iter()
                .position(|candidate| candidate.task_id == task.task_id)
                .unwrap_or_default();
            if task_index == selected_task_index.load(Ordering::Relaxed) {
                initial_selected_idx = Some(items.len());
            }
            items.push(task_item(thread_id, task, &tree_prefix));
            item_task_indices.push(Some(task_index));
        }
        if !tasks.is_empty() {
            items.push(SelectionItem {
                name: "当前任务".to_string(),
                description: Some("对高亮的任务节点执行操作".to_string()),
                is_disabled: true,
                ..Default::default()
            });
            item_task_indices.push(None);
            items.push(selected_task_conversation_item(
                thread_id,
                Arc::clone(&tasks),
                Arc::clone(&selected_task_index),
            ));
            item_task_indices.push(None);
            for (name, description, action) in [
                (
                    "  追加要求",
                    "补充约束或验收条件",
                    selected_task_action(ProjectAgentWorkbenchAction::PromptAppendRequirement),
                ),
                (
                    "  创建子任务",
                    "在当前节点下建立任务",
                    selected_task_action(ProjectAgentWorkbenchAction::PromptCreateChild),
                ),
                (
                    "  选择执行者",
                    "把当前节点交给项目 AGENT",
                    selected_task_action(ProjectAgentWorkbenchAction::PromptStartExecution),
                ),
                (
                    "  刷新",
                    "重新读取任务、结果和评测",
                    selected_task_action(ProjectAgentWorkbenchAction::Refresh),
                ),
            ] {
                items.push(selected_task_action_item(
                    thread_id,
                    name,
                    description,
                    Arc::clone(&tasks),
                    Arc::clone(&selected_task_index),
                    action,
                ));
                item_task_indices.push(None);
            }
        }

        let item_task_indices = Arc::new(item_task_indices);
        let selection_index = Arc::clone(&selected_task_index);
        let params = SelectionViewParams {
            view_id: Some(PROJECT_TASK_WORKBENCH_VIEW_ID),
            header: Box::new(header),
            items,
            initial_selected_idx,
            is_searchable: true,
            search_placeholder: Some("搜索标题或 task id".to_string()),
            col_width_mode: ColumnWidthMode::AutoAllRows,
            side_content: Box::new(SelectedTaskDetailPane {
                tasks,
                agents,
                selected_task_index: Arc::clone(&selected_task_index),
            }),
            side_content_width: SideContentWidth::Half,
            side_content_min_width: 42,
            on_selection_changed: Some(Box::new(move |actual_index, _tx| {
                if let Some(task_index) = item_task_indices.get(actual_index).copied().flatten() {
                    selection_index.store(task_index, Ordering::Relaxed);
                }
            })),
            footer_note: Some(Line::from("任务和执行记录保留在同一个工作区".dim())),
            ..Default::default()
        };
        if self.bottom_pane.active_view_id() == Some(PROJECT_TASK_WORKBENCH_VIEW_ID) {
            let _ = self
                .bottom_pane
                .replace_selection_view_if_active(PROJECT_TASK_WORKBENCH_VIEW_ID, params);
        } else {
            self.bottom_pane.show_selection_view(params);
        }
    }

    pub(crate) fn show_project_task_create_prompt(
        &mut self,
        thread_id: ThreadId,
        parent_task_id: Option<String>,
    ) {
        let tx = self.app_event_tx.clone();
        let context = parent_task_id
            .as_ref()
            .map(|task_id| format!("父任务：{task_id}"))
            .unwrap_or_else(|| "新建根任务".to_string());
        let view = CustomPromptView::new(
            "创建项目任务".to_string(),
            "第一行标题；后续内容作为目标".to_string(),
            String::new(),
            Some(context),
            Box::new(move |input| {
                tx.send(AppEvent::ProjectAgentWorkbench {
                    thread_id,
                    action: ProjectAgentWorkbenchAction::Create {
                        parent_task_id: parent_task_id.clone(),
                        input,
                    },
                });
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    pub(crate) fn show_project_task_requirement_prompt(
        &mut self,
        thread_id: ThreadId,
        task_id: String,
    ) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "追加任务要求".to_string(),
            "补充约束或验收要求".to_string(),
            String::new(),
            Some(format!("任务：{task_id}")),
            Box::new(move |requirement| {
                tx.send(AppEvent::ProjectAgentWorkbench {
                    thread_id,
                    action: ProjectAgentWorkbenchAction::AppendRequirement {
                        task_id: task_id.clone(),
                        requirement,
                    },
                });
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    pub(crate) fn show_project_task_executor_picker(
        &mut self,
        thread_id: ThreadId,
        task_id: String,
        agents: Vec<ProjectAgentRosterEntry>,
    ) {
        let mut header = ColumnRenderable::new();
        header.push(Line::from("选择项目 AGENT 执行者".cyan().bold()));
        header.push(Line::from(format!("任务：{task_id}").dim()));
        let items = agents
            .into_iter()
            .map(|agent| {
                let agent_id = agent.id;
                let action_agent_id = agent_id.clone();
                let action_task_id = task_id.clone();
                SelectionItem {
                    name: format!("@{agent_id}"),
                    description: Some(agent.description),
                    disabled_reason: (!agent.enabled).then(|| "此 AGENT 已禁用。".to_string()),
                    actions: vec![Box::new(move |tx| {
                        tx.send(AppEvent::ProjectAgentWorkbench {
                            thread_id,
                            action: ProjectAgentWorkbenchAction::StartExecution {
                                task_id: action_task_id.clone(),
                                agent_id: action_agent_id.clone(),
                            },
                        });
                    })],
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();
        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            items,
            is_searchable: true,
            search_placeholder: Some("搜索 @agent".to_string()),
            col_width_mode: ColumnWidthMode::AutoAllRows,
            ..Default::default()
        });
    }
}

fn task_tree(tasks: &[ProjectTask]) -> Vec<(&ProjectTask, String)> {
    let by_parent = tasks.iter().fold(
        HashMap::<Option<&str>, Vec<&ProjectTask>>::new(),
        |mut index, task| {
            index
                .entry(task.parent_task_id.as_deref())
                .or_default()
                .push(task);
            index
        },
    );
    let mut ordered = Vec::new();
    append_children(&by_parent, None, "", &mut ordered);
    ordered
}

fn append_children<'a>(
    by_parent: &HashMap<Option<&'a str>, Vec<&'a ProjectTask>>,
    parent: Option<&'a str>,
    prefix: &str,
    ordered: &mut Vec<(&'a ProjectTask, String)>,
) {
    let Some(children) = by_parent.get(&parent) else {
        return;
    };
    for (index, task) in children.iter().enumerate() {
        let is_last = index + 1 == children.len();
        let connector = match (parent, is_last) {
            (None, _) => "",
            (Some(_), false) => "├─ ",
            (Some(_), true) => "└─ ",
        };
        ordered.push((task, format!("{prefix}{connector}")));
        let child_prefix = if parent.is_none() {
            String::new()
        } else if is_last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}│  ")
        };
        append_children(
            by_parent,
            Some(task.task_id.as_str()),
            &child_prefix,
            ordered,
        );
    }
}

fn task_item(thread_id: ThreadId, task: &ProjectTask, tree_prefix: &str) -> SelectionItem {
    let status = task_status_label(task.status);
    let action = project_task_open_conversation_action(thread_id, task);
    let dismiss_on_select = action.is_some();
    SelectionItem {
        name: format!("{tree_prefix}{}", task.title),
        name_prefix_spans: vec![task_status_symbol(task.status)],
        description: Some(format!("{status} · {}", executor_label(&task.executor))),
        search_value: Some(format!("{} {}", task.task_id, task.title)),
        actions: action.into_iter().collect(),
        disabled_reason: project_task_conversation_unavailable_reason(task),
        dismiss_on_select,
        ..Default::default()
    }
}

fn task_detail(
    task: &ProjectTask,
    agents: &[ProjectAgentRosterEntry],
) -> ColumnRenderable<'static> {
    let mut detail = ColumnRenderable::new();
    detail.push(Line::from(task.title.clone().bold()));
    detail.push(Line::from(format!("task {}", task.task_id).dim()));
    detail.push(Line::from(format!(
        "{} · {}",
        task_status_label(task.status),
        executor_label(&task.executor)
    )));
    detail.push(Line::from(""));
    detail.push(Line::from("目标".cyan().bold()));
    detail.push(Line::from(truncate_text(&task.objective, 600)));
    if !task.requirements.is_empty() {
        detail.push(Line::from(""));
        detail.push(Line::from("追加要求".cyan().bold()));
        for requirement in &task.requirements {
            detail.push(Line::from(format!("• {requirement}")));
        }
    }
    detail.push(Line::from(""));
    detail.push(Line::from("执行流".cyan().bold()));
    detail.push(Line::from(execution_flow(task)));
    if let Some(result) = task.result.as_ref() {
        detail.push(Line::from(""));
        detail.push(
            Line::from(format!("结果 · {}", result_status_label(result.status)))
                .cyan()
                .bold(),
        );
        detail.push(Line::from(result.summary.clone()));
        for evidence in &result.evidence {
            detail.push(Line::from(format!("证据 · {evidence}").dim()));
        }
        for artifact in &result.artifacts {
            detail.push(Line::from(format!("产物 · {artifact}").dim()));
        }
        if let Some(error) = result.error.as_deref() {
            detail.push(Line::from(format!("错误 · {error}").red()));
        }
        if !result.suggested_children.is_empty() {
            detail.push(Line::from("建议子任务".cyan().bold()));
            for child in &result.suggested_children {
                detail.push(Line::from(format!(
                    "• {} · {}",
                    child.title, child.objective
                )));
            }
        }
    }
    if let Some(evaluation) = task.evaluation.as_ref() {
        detail.push(Line::from(""));
        detail.push(
            Line::from(format!("评测 · {}", evaluation_label(evaluation.verdict)))
                .cyan()
                .bold(),
        );
        detail.push(Line::from(evaluation.summary.clone()));
        for evidence in &evaluation.evidence {
            detail.push(Line::from(format!("评测证据 · {evidence}").dim()));
        }
    }
    detail.push(Line::from(
        format!(
            "可用执行者：{}",
            agents
                .iter()
                .filter(|agent| agent.enabled)
                .map(|agent| format!("@{}", agent.id))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .dim(),
    ));
    detail
}

fn empty_task_detail(agents: &[ProjectAgentRosterEntry]) -> ColumnRenderable<'static> {
    let mut detail = ColumnRenderable::new();
    detail.push(Line::from("尚无任务".bold()));
    detail.push(Line::from("从左侧“新建根任务”开始。".dim()));
    detail.push(Line::from(format!("已注册执行者：{}", agents.len()).dim()));
    detail
}

fn action_item(
    thread_id: ThreadId,
    name: &str,
    description: &str,
    action: ProjectAgentWorkbenchAction,
) -> SelectionItem {
    SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        actions: vec![Box::new(move |tx| {
            tx.send(AppEvent::ProjectAgentWorkbench {
                thread_id,
                action: action.clone(),
            });
        })],
        dismiss_on_select: true,
        ..Default::default()
    }
}

type TaskActionFactory = Arc<dyn Fn(String) -> ProjectAgentWorkbenchAction + Send + Sync>;

fn selected_task_conversation_item(
    thread_id: ThreadId,
    tasks: Arc<Vec<ProjectTask>>,
    selected_task_index: Arc<AtomicUsize>,
) -> SelectionItem {
    let disabled_reason = tasks
        .get(selected_task_index.load(Ordering::Relaxed))
        .and_then(project_task_conversation_unavailable_reason);
    SelectionItem {
        name: "  打开会话".to_string(),
        description: Some("进入该任务的完整项目 AGENT 对话".to_string()),
        actions: vec![Box::new(move |tx| {
            let Some(task) = tasks.get(selected_task_index.load(Ordering::Relaxed)) else {
                return;
            };
            send_project_task_open_conversation(tx, thread_id, task);
        })],
        disabled_reason,
        dismiss_on_select: true,
        ..Default::default()
    }
}

fn project_task_open_conversation_action(
    thread_id: ThreadId,
    task: &ProjectTask,
) -> Option<SelectionAction> {
    project_task_conversation_unavailable_reason(task)
        .is_none()
        .then(|| {
            let task = task.clone();
            Box::new(move |tx: &crate::app_event_sender::AppEventSender| {
                send_project_task_open_conversation(tx, thread_id, &task);
            }) as SelectionAction
        })
}

fn send_project_task_open_conversation(
    tx: &crate::app_event_sender::AppEventSender,
    thread_id: ThreadId,
    task: &ProjectTask,
) {
    let ProjectTaskExecutor::ProjectAgent { agent_id } = &task.executor else {
        return;
    };
    let Some(session_thread_id) = task.session_thread_id.clone() else {
        return;
    };
    tx.send(AppEvent::ProjectAgentWorkbench {
        thread_id,
        action: ProjectAgentWorkbenchAction::OpenConversation {
            task_id: task.task_id.clone(),
            task_title: task.title.clone(),
            agent_id: agent_id.clone(),
            session_thread_id,
        },
    });
}

fn project_task_conversation_unavailable_reason(task: &ProjectTask) -> Option<String> {
    match (&task.executor, &task.session_thread_id) {
        (ProjectTaskExecutor::ProjectAgent { .. }, Some(_)) => None,
        (ProjectTaskExecutor::ProjectAgent { .. }, None) => {
            Some("该任务尚未启动或 worker 会话尚未绑定".to_string())
        }
        _ => Some("该任务尚未分配项目 AGENT".to_string()),
    }
}

fn selected_task_action(
    factory: impl Fn(String) -> ProjectAgentWorkbenchAction + Send + Sync + 'static,
) -> TaskActionFactory {
    Arc::new(factory)
}

fn selected_task_action_item(
    thread_id: ThreadId,
    name: &str,
    description: &str,
    tasks: Arc<Vec<ProjectTask>>,
    selected_task_index: Arc<AtomicUsize>,
    action: TaskActionFactory,
) -> SelectionItem {
    SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        search_value: Some(format!("{name} {description}")),
        actions: vec![Box::new(move |tx| {
            let task_index = selected_task_index.load(Ordering::Relaxed);
            if let Some(task) = tasks.get(task_index) {
                tx.send(AppEvent::ProjectAgentWorkbench {
                    thread_id,
                    action: action(task.task_id.clone()),
                });
            }
        })],
        dismiss_on_select: true,
        ..Default::default()
    }
}

struct SelectedTaskDetailPane {
    tasks: Arc<Vec<ProjectTask>>,
    agents: Arc<Vec<ProjectAgentRosterEntry>>,
    selected_task_index: Arc<AtomicUsize>,
}

impl SelectedTaskDetailPane {
    fn content(&self) -> ColumnRenderable<'static> {
        self.tasks
            .get(self.selected_task_index.load(Ordering::Relaxed))
            .map(|task| task_detail(task, &self.agents))
            .unwrap_or_else(|| empty_task_detail(&self.agents))
    }
}

impl Renderable for SelectedTaskDetailPane {
    fn render(&self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        self.content().render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.content().desired_height(width)
    }
}

fn execution_flow(task: &ProjectTask) -> String {
    match (&task.execution_task_id, task.status) {
        (Some(execution_task_id), ProjectTaskStatus::InProgress) => {
            format!("执行中 · internal {execution_task_id}")
        }
        (Some(execution_task_id), status) => {
            format!(
                "{} · internal {execution_task_id}",
                task_status_label(status)
            )
        }
        (None, ProjectTaskStatus::Pending) => "等待分配执行者".to_string(),
        (None, status) => task_status_label(status).to_string(),
    }
}

fn executor_label(executor: &ProjectTaskExecutor) -> String {
    match executor {
        ProjectTaskExecutor::Unassigned => "未分配".to_string(),
        ProjectTaskExecutor::MainAgent => "主 AGENT".to_string(),
        ProjectTaskExecutor::ProjectAgent { agent_id } => format!("@{agent_id}"),
    }
}

fn task_status_symbol(status: ProjectTaskStatus) -> ratatui::text::Span<'static> {
    match status {
        ProjectTaskStatus::Pending => "○ ".dim(),
        ProjectTaskStatus::InProgress => "● ".cyan(),
        ProjectTaskStatus::Completed => "✓ ".green(),
        ProjectTaskStatus::Rejected => "× ".magenta(),
        ProjectTaskStatus::Blocked => "■ ".magenta(),
        ProjectTaskStatus::Failed => "! ".red(),
    }
}

fn task_status_label(status: ProjectTaskStatus) -> &'static str {
    match status {
        ProjectTaskStatus::Pending => "待执行",
        ProjectTaskStatus::InProgress => "执行中",
        ProjectTaskStatus::Completed => "已完成",
        ProjectTaskStatus::Rejected => "已拒绝",
        ProjectTaskStatus::Blocked => "已阻塞",
        ProjectTaskStatus::Failed => "失败",
    }
}

fn evaluation_label(verdict: ProjectTaskEvaluationVerdict) -> &'static str {
    match verdict {
        ProjectTaskEvaluationVerdict::Passed => "通过",
        ProjectTaskEvaluationVerdict::NeedsRevision => "需修订",
        ProjectTaskEvaluationVerdict::Failed => "未通过",
    }
}

fn result_status_label(status: ProjectTaskResultStatus) -> &'static str {
    match status {
        ProjectTaskResultStatus::Completed => "已完成",
        ProjectTaskResultStatus::Rejected => "已拒绝",
        ProjectTaskResultStatus::BlockedMissingTool => "缺少工具",
        ProjectTaskResultStatus::Failed => "失败",
    }
}
