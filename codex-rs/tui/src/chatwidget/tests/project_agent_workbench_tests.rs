use super::*;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;
use codex_app_server_protocol::ProjectAgentDefinition;
use codex_app_server_protocol::ProjectAgentPersistedResult;
use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_app_server_protocol::ProjectAgentSession;
use codex_app_server_protocol::ProjectAgentTask;
use codex_app_server_protocol::ProjectAgentTaskPhase;
use codex_app_server_protocol::ProjectAgentTaskStatus;
use codex_app_server_protocol::ThreadProjectAgentListResponse;
use codex_app_server_protocol::ThreadProjectAgentReadResponse;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn agents_commands_target_the_current_thread() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.dispatch_command(SlashCommand::Agents);
    assert_eq!(
        next_workbench_action(&mut rx, thread_id),
        ProjectAgentWorkbenchAction::List
    );

    chat.dispatch_command_with_args(SlashCommand::Agents, "@query".to_string(), Vec::new());
    assert_eq!(
        next_workbench_action(&mut rx, thread_id),
        ProjectAgentWorkbenchAction::Read("query".to_string())
    );
}

#[tokio::test]
async fn project_agent_roster_popup_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();

    chat.show_project_agent_roster(
        thread_id,
        ThreadProjectAgentListResponse {
            project_root: "/workspace/demo".to_string(),
            data: vec![ProjectAgentRosterEntry {
                id: "query".to_string(),
                description: "查询项目数据并返回带证据的结论".to_string(),
                enabled: true,
                active_session_thread_id: Some("worker-query".to_string()),
                session: Some(test_session()),
                current_task: Some(test_task(ProjectAgentTaskPhase::Running)),
            }],
            next_cursor: None,
            total: 1,
        },
    );

    let popup = render_bottom_popup(&chat, /*width*/ 120);
    insta::assert_snapshot!(
        popup_lines(
            &popup,
            &["项目 AGENT 工作台", "共 1 个", "@query"],
        ),
        @r###"
    项目 AGENT 工作台
    共 1 个；输入 @名字搜索，Enter 打开详情。
    @query 运行中 · 核对当前项目的索引状态，并把证据和异常一起返回。
    "###
    );
}

#[tokio::test]
async fn project_agent_detail_popup_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();

    chat.show_project_agent_detail(
        thread_id,
        test_read_response(ProjectAgentTaskPhase::Running),
    );

    let popup = render_bottom_popup(&chat, /*width*/ 160);
    insta::assert_snapshot!(
        popup_lines(
            &popup,
            &[
                "项目 AGENT · @query",
                "会话：",
                "当前任务",
                "持久化响应",
                "追加跟进",
                "终止当前任务",
            ],
        ),
        @r###"
    项目 AGENT · @query
    会话：第 2 代 · worker-query
    当前任务 执行中 · 第 1 次 · 核对当前项目的索引状态，并把证据和异常一起返回。 · 父线程 parent-thread
    持久化响应 已完成 · 索引已核对。 发现一条需要主 AGENT 决策的异常。
    追加跟进 向正在执行的专职任务补充上下文。
    终止当前任务 中断当前专职任务并保留可重试的持久化状态。
    "###
    );
}

#[tokio::test]
async fn project_agent_controls_emit_typed_actions() {
    let thread_id = ThreadId::new();
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.show_project_agent_detail(
        thread_id,
        test_read_response(ProjectAgentTaskPhase::Interrupted),
    );
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));
    assert_eq!(
        next_workbench_action(&mut rx, thread_id),
        ProjectAgentWorkbenchAction::Retry {
            agent_id: "query".to_string(),
            task_id: "task-1".to_string(),
        }
    );
    chat.show_project_agent_follow_up_prompt(thread_id, "query".to_string());

    for ch in "add evidence".chars() {
        chat.handle_key_event(KeyEvent::from(KeyCode::Char(ch)));
    }
    chat.handle_key_event(KeyEvent::from(KeyCode::Enter));

    assert_eq!(
        next_workbench_action(&mut rx, thread_id),
        ProjectAgentWorkbenchAction::FollowUp {
            agent_id: "query".to_string(),
            message: "add evidence".to_string(),
        }
    );
}

fn next_workbench_action(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    expected_thread_id: ThreadId,
) -> ProjectAgentWorkbenchAction {
    match rx
        .try_recv()
        .expect("expected project AGENT workbench event")
    {
        AppEvent::ProjectAgentWorkbench { thread_id, action } => {
            assert_eq!(thread_id, expected_thread_id);
            action
        }
        event => panic!("expected project AGENT workbench event, got {event:?}"),
    }
}

fn popup_lines(popup: &str, needles: &[&str]) -> String {
    needles
        .iter()
        .map(|needle| {
            let line = popup
                .lines()
                .find(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("expected popup line containing {needle:?}:\n{popup}"));
            let start = line.find(needle).expect("needle should be present");
            line[start..]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn test_read_response(phase: ProjectAgentTaskPhase) -> ThreadProjectAgentReadResponse {
    let task = test_task(phase);
    ThreadProjectAgentReadResponse {
        project_root: "/workspace/demo".to_string(),
        agent: ProjectAgentDefinition {
            id: "query".to_string(),
            description: "查询项目数据并返回带证据的结论".to_string(),
            enabled: true,
            definition_path: "AGENT/query/AGENT.md".to_string(),
            constraints_file: "constraints.md".to_string(),
            model: Some("gpt-5".to_string()),
            model_reasoning_effort: Some("high".to_string()),
            model_provider: None,
            tools: vec!["search".to_string(), "read".to_string()],
            memory_max_items: 8,
            memory_max_tokens: 1200,
        },
        active_session_thread_id: Some("worker-query".to_string()),
        session: Some(test_session()),
        current_task: Some(task.clone()),
        current_result: (!matches!(phase, ProjectAgentTaskPhase::Running)).then(test_result),
        recent_tasks: vec![task],
        recent_results: vec![test_result()],
    }
}

fn test_session() -> ProjectAgentSession {
    ProjectAgentSession {
        agent_id: "query".to_string(),
        thread_id: "worker-query".to_string(),
        generation: 2,
        created_at: 10,
        updated_at: 20,
    }
}

fn test_task(phase: ProjectAgentTaskPhase) -> ProjectAgentTask {
    ProjectAgentTask {
        agent_id: "query".to_string(),
        task_id: "task-1".to_string(),
        task: "核对当前项目的索引状态，并把证据和异常一起返回。".to_string(),
        phase,
        session_thread_id: Some("worker-query".to_string()),
        parent_thread_id: "parent-thread".to_string(),
        attempt: 1,
        created_at: 10,
        updated_at: 20,
    }
}

fn test_result() -> ProjectAgentPersistedResult {
    ProjectAgentPersistedResult {
        status: ProjectAgentTaskStatus::Completed,
        agent_id: "query".to_string(),
        task_id: "task-1".to_string(),
        result: "索引已核对。\n发现一条需要主 AGENT 决策的异常。".to_string(),
        artifacts: vec!["task/artifacts/index-report.md".to_string()],
        evidence: vec!["index.json:12".to_string()],
        memory_candidates: Vec::new(),
        improvement_proposals: Vec::new(),
        error: None,
    }
}
