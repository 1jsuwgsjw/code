use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::ColumnWidthMode;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;
use crate::render::renderable::ColumnRenderable;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::ProjectAgentPersistedResult;
use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_app_server_protocol::ProjectAgentTask;
use codex_app_server_protocol::ProjectAgentTaskPhase;
use codex_app_server_protocol::ProjectAgentTaskStatus;
use codex_app_server_protocol::ThreadProjectAgentListResponse;
use codex_app_server_protocol::ThreadProjectAgentReadResponse;
use codex_protocol::ThreadId;
use ratatui::style::Stylize;
use ratatui::text::Line;

const PROJECT_AGENT_WORKBENCH_VIEW_ID: &str = "project-agent-workbench";
const PROJECT_AGENT_USAGE: &str = "用法：/agents 或 /agents @agent-name";

impl ChatWidget {
    pub(super) fn dispatch_project_agent_workbench(&mut self, args: &str) {
        let Some(thread_id) = self.thread_id else {
            self.add_error_message("会话启动后才能打开项目 AGENT 工作台。".to_string());
            return;
        };
        let trimmed = args.trim();
        let action = match trimmed.strip_prefix('@') {
            None if trimmed.is_empty() => ProjectAgentWorkbenchAction::List,
            Some(agent_id) if !agent_id.is_empty() && !agent_id.contains(char::is_whitespace) => {
                ProjectAgentWorkbenchAction::Read(agent_id.to_string())
            }
            _ => {
                self.add_error_message(PROJECT_AGENT_USAGE.to_string());
                return;
            }
        };
        self.app_event_tx
            .send(AppEvent::ProjectAgentWorkbench { thread_id, action });
    }

    pub(crate) fn show_project_agent_roster(
        &mut self,
        thread_id: ThreadId,
        response: ThreadProjectAgentListResponse,
    ) {
        let ThreadProjectAgentListResponse {
            project_root,
            data,
            next_cursor,
            total,
        } = response;
        let shown = data.len();
        let mut header = ColumnRenderable::new();
        header.push(Line::from("项目 AGENT 工作台".cyan().bold()));
        header.push(Line::from(format!("项目：{project_root}").dim()));
        header.push(Line::from(
            format!("共 {total} 个；输入 @名字搜索，Enter 打开详情。").dim(),
        ));
        if next_cursor.is_some() {
            header.push(Line::from(
                format!("当前显示前 {shown} 个，更多结果暂未加载。").yellow(),
            ));
        }

        let mut items = data
            .into_iter()
            .map(|entry| roster_item(thread_id, entry))
            .collect::<Vec<_>>();
        if items.is_empty() {
            items.push(SelectionItem {
                name: "未发现已注册的项目 AGENT".to_string(),
                description: Some("请检查项目根目录下的 AGENT/registry.toml。".to_string()),
                is_disabled: true,
                ..Default::default()
            });
        }
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(PROJECT_AGENT_WORKBENCH_VIEW_ID),
            header: Box::new(header),
            items,
            is_searchable: true,
            search_placeholder: Some("输入 @agent-name 搜索".to_string()),
            col_width_mode: ColumnWidthMode::AutoAllRows,
            ..Default::default()
        });
    }

    pub(crate) fn show_project_agent_detail(
        &mut self,
        thread_id: ThreadId,
        response: ThreadProjectAgentReadResponse,
    ) {
        let ThreadProjectAgentReadResponse {
            project_root,
            agent,
            active_session_thread_id,
            session,
            current_task,
            current_result,
            recent_tasks,
            recent_results,
        } = response;
        let agent_id = agent.id.clone();
        let task_is_active = current_task
            .as_ref()
            .is_some_and(|task| is_active_task(task.phase));
        let latest_task = current_task.as_ref().or_else(|| recent_tasks.first());
        let latest_result = current_result.as_ref().or_else(|| recent_results.first());
        let active_action_disabled = (!agent.enabled || !task_is_active)
            .then(|| "仅已启用且正在执行的 AGENT 可使用此操作。".to_string());
        let idle_action_disabled = if !agent.enabled {
            Some("此 AGENT 已禁用。".to_string())
        } else {
            task_is_active.then(|| "活动任务结束后才能执行此操作。".to_string())
        };

        let mut header = ColumnRenderable::new();
        header.push(Line::from(
            format!("项目 AGENT · @{agent_id}").cyan().bold(),
        ));
        header.push(Line::from(truncate_text(
            &agent.description,
            /*max_graphemes*/ 180,
        )));
        header.push(Line::from(format!("项目：{project_root}").dim()));
        let session_label = session
            .as_ref()
            .map(|session| format!("第 {} 代 · {}", session.generation, session.thread_id))
            .or(active_session_thread_id)
            .unwrap_or_else(|| "尚未创建".to_string());
        header.push(Line::from(format!("会话：{session_label}").dim()));

        let mut items = vec![
            detail_item(
                if task_is_active {
                    "当前任务"
                } else {
                    "最近任务"
                },
                task_preview(latest_task),
            ),
            detail_item("持久化响应", result_preview(latest_result)),
        ];
        items.push(action_item(
            thread_id,
            "追加跟进",
            "向正在执行的专职任务补充上下文。",
            active_action_disabled.clone(),
            ProjectAgentWorkbenchAction::PromptFollowUp(agent_id.clone()),
        ));
        items.push(action_item(
            thread_id,
            "终止当前任务",
            "中断当前专职任务并保留可重试的持久化状态。",
            active_action_disabled,
            ProjectAgentWorkbenchAction::Terminate(agent_id.clone()),
        ));
        if let Some(task) = latest_task {
            items.push(action_item(
                thread_id,
                "重试此任务",
                "在专职会话中创建一次新的任务尝试。",
                idle_action_disabled.clone(),
                ProjectAgentWorkbenchAction::Retry {
                    agent_id: agent_id.clone(),
                    task_id: task.task_id.clone(),
                },
            ));
        }
        items.push(action_item(
            thread_id,
            "重建专职会话",
            "丢弃旧会话上下文并创建新一代隔离会话。",
            idle_action_disabled,
            ProjectAgentWorkbenchAction::Rebuild(agent_id.clone()),
        ));
        items.push(action_item(
            thread_id,
            "返回 AGENT 列表",
            "重新加载当前项目的专职 AGENT 名单。",
            /*disabled_reason*/ None,
            ProjectAgentWorkbenchAction::List,
        ));

        let initial_selected_idx = items
            .iter()
            .position(|item| !item.is_disabled && item.disabled_reason.is_none());
        self.bottom_pane.show_selection_view(SelectionViewParams {
            view_id: Some(PROJECT_AGENT_WORKBENCH_VIEW_ID),
            header: Box::new(header),
            footer_note: Some(Line::from("/agents @名字 可直接打开此详情。".dim())),
            items,
            initial_selected_idx,
            col_width_mode: ColumnWidthMode::AutoAllRows,
            ..Default::default()
        });
    }

    pub(crate) fn show_project_agent_follow_up_prompt(
        &mut self,
        thread_id: ThreadId,
        agent_id: String,
    ) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "追加项目 AGENT 跟进".to_string(),
            "输入补充要求并按 Enter 发送".to_string(),
            /*initial_text*/ String::new(),
            /*context_label*/ Some(format!("目标：@{agent_id}")),
            Box::new(move |message| {
                tx.send(AppEvent::ProjectAgentWorkbench {
                    thread_id,
                    action: ProjectAgentWorkbenchAction::FollowUp {
                        agent_id: agent_id.clone(),
                        message,
                    },
                });
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }
}

fn detail_item(name: &str, description: String) -> SelectionItem {
    SelectionItem {
        name: name.to_string(),
        description: Some(description),
        is_disabled: true,
        ..Default::default()
    }
}

fn action_item(
    thread_id: ThreadId,
    name: &str,
    description: &str,
    disabled_reason: Option<String>,
    action: ProjectAgentWorkbenchAction,
) -> SelectionItem {
    SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        disabled_reason,
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

fn roster_item(thread_id: ThreadId, entry: ProjectAgentRosterEntry) -> SelectionItem {
    let active = entry
        .current_task
        .as_ref()
        .is_some_and(|task| is_active_task(task.phase));
    let (status, prefix) = if !entry.enabled {
        ("已禁用", "× ".dim())
    } else if active {
        ("运行中", "● ".green())
    } else if entry.session.is_some() {
        ("待命", "○ ".cyan())
    } else {
        ("未启动", "○ ".cyan())
    };
    let agent_id = entry.id;
    let action_agent_id = agent_id.clone();
    let summary = entry
        .current_task
        .as_ref()
        .map(|task| truncate_text(&task.task, /*max_graphemes*/ 120))
        .unwrap_or_else(|| truncate_text(&entry.description, /*max_graphemes*/ 120));
    SelectionItem {
        name: format!("@{agent_id}"),
        name_prefix_spans: vec![prefix],
        description: Some(format!("{status} · {summary}")),
        search_value: Some(format!("@{agent_id} {agent_id}")),
        actions: vec![Box::new(move |tx| {
            tx.send(AppEvent::ProjectAgentWorkbench {
                thread_id,
                action: ProjectAgentWorkbenchAction::Read(action_agent_id.clone()),
            });
        })],
        dismiss_on_select: true,
        ..Default::default()
    }
}

fn is_active_task(phase: ProjectAgentTaskPhase) -> bool {
    matches!(
        phase,
        ProjectAgentTaskPhase::Queued | ProjectAgentTaskPhase::Running
    )
}

fn task_preview(task: Option<&ProjectAgentTask>) -> String {
    task.map(|task| {
        format!(
            "{} · 第 {} 次 · {} · 父线程 {}",
            task_phase_label(task.phase),
            task.attempt,
            truncate_text(&task.task, /*max_graphemes*/ 200),
            task.parent_thread_id
        )
    })
    .unwrap_or_else(|| "暂无任务记录".to_string())
}

fn result_preview(result: Option<&ProjectAgentPersistedResult>) -> String {
    let Some(result) = result else {
        return "暂无持久化响应".to_string();
    };
    let text = result.result.replace('\r', " ").replace('\n', " ");
    let preview = if text.trim().is_empty() {
        result.error.as_deref().unwrap_or("未返回正文")
    } else {
        text.trim()
    };
    format!(
        "{} · {}",
        task_status_label(result.status),
        truncate_text(preview, /*max_graphemes*/ 320)
    )
}

fn task_phase_label(phase: ProjectAgentTaskPhase) -> &'static str {
    match phase {
        ProjectAgentTaskPhase::Queued => "排队中",
        ProjectAgentTaskPhase::Running => "执行中",
        ProjectAgentTaskPhase::Completed => "已完成",
        ProjectAgentTaskPhase::Interrupted => "已中断",
        ProjectAgentTaskPhase::Failed => "失败",
    }
}

fn task_status_label(status: ProjectAgentTaskStatus) -> &'static str {
    match status {
        ProjectAgentTaskStatus::Completed => "已完成",
        ProjectAgentTaskStatus::RejectedOutOfScope => "拒绝：超出职责",
        ProjectAgentTaskStatus::BlockedMissingTool => "阻塞：缺少工具",
        ProjectAgentTaskStatus::Failed => "失败",
    }
}
