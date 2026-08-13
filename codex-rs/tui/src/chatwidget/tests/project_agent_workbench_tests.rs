use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_app_server_protocol::ProjectTask;
use codex_app_server_protocol::ProjectTaskEvaluation;
use codex_app_server_protocol::ProjectTaskEvaluationVerdict;
use codex_app_server_protocol::ProjectTaskExecutor;
use codex_app_server_protocol::ProjectTaskResult;
use codex_app_server_protocol::ProjectTaskResultStatus;
use codex_app_server_protocol::ProjectTaskStatus;
use codex_app_server_protocol::ProjectTaskSuggestedChild;
use codex_app_server_protocol::ProjectTaskWorkspace;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;

use super::*;
use crate::project_agent_workbench::ProjectAgentWorkbenchAction;

#[tokio::test]
async fn agent_command_opens_semantic_task_workspace() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.dispatch_command(SlashCommand::Agents);

    assert_eq!(
        next_workbench_action(&mut rx),
        (
            thread_id,
            ProjectAgentWorkbenchAction::Open {
                selected_task_id: None
            }
        )
    );
}

#[tokio::test]
async fn agent_command_accepts_task_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.dispatch_command_with_args(SlashCommand::Agents, "task-root".to_string(), Vec::new());

    assert_eq!(
        next_workbench_action(&mut rx),
        (
            thread_id,
            ProjectAgentWorkbenchAction::Open {
                selected_task_id: Some("task-root".to_string())
            }
        )
    );
}

#[tokio::test]
async fn task_workspace_renders_tree_and_selected_task_detail_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.show_project_task_workspace(
        thread_id,
        test_workspace(),
        test_agents(),
        Some("task-child".to_string()),
    );

    assert_snapshot!(
        "project_task_workspace_tree",
        render_bottom_popup(&chat, /*width*/ 150)
    );
}

#[tokio::test]
async fn task_workspace_empty_state_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.show_project_task_workspace(
        ThreadId::new(),
        ProjectTaskWorkspace {
            schema_version: 1,
            tasks: Vec::new(),
            updated_at: 0,
        },
        test_agents(),
        None,
    );

    assert_snapshot!(
        "project_task_workspace_empty",
        render_bottom_popup(&chat, /*width*/ 120)
    );
}

#[tokio::test]
async fn create_and_requirement_prompts_emit_semantic_task_actions() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.show_project_task_create_prompt(thread_id, Some("task-root".to_string()));
    chat.bottom_pane
        .handle_paste("Implement UI\nRender the semantic task tree".to_string());
    chat.bottom_pane
        .handle_key_event(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Enter,
        ));
    assert_eq!(
        next_workbench_action(&mut rx),
        (
            thread_id,
            ProjectAgentWorkbenchAction::Create {
                parent_task_id: Some("task-root".to_string()),
                input: "Implement UI\nRender the semantic task tree".to_string(),
            }
        )
    );

    chat.show_project_task_requirement_prompt(thread_id, "task-root".to_string());
    chat.bottom_pane
        .handle_paste("Include snapshot coverage".to_string());
    chat.bottom_pane
        .handle_key_event(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Enter,
        ));
    assert_eq!(
        next_workbench_action(&mut rx),
        (
            thread_id,
            ProjectAgentWorkbenchAction::AppendRequirement {
                task_id: "task-root".to_string(),
                requirement: "Include snapshot coverage".to_string(),
            }
        )
    );
}

fn next_workbench_action(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (ThreadId, ProjectAgentWorkbenchAction) {
    loop {
        match rx.try_recv().expect("workbench event") {
            AppEvent::ProjectAgentWorkbench { thread_id, action } => return (thread_id, action),
            _ => continue,
        }
    }
}

fn test_workspace() -> ProjectTaskWorkspace {
    ProjectTaskWorkspace {
        schema_version: 1,
        updated_at: 20,
        tasks: vec![
            ProjectTask {
                task_id: "task-root".to_string(),
                parent_task_id: None,
                title: "Ship task workspace".to_string(),
                objective: "Expose durable semantic tasks through app-server and TUI.".to_string(),
                requirements: vec!["Keep task identity stable".to_string()],
                status: ProjectTaskStatus::InProgress,
                executor: ProjectTaskExecutor::ProjectAgent {
                    agent_id: "query".to_string(),
                },
                execution_task_id: Some("worker-019".to_string()),
                session_thread_id: Some("01900000-0000-7000-8000-000000000019".to_string()),
                result: None,
                evaluation: None,
                created_at: 10,
                updated_at: 15,
            },
            ProjectTask {
                task_id: "task-child".to_string(),
                parent_task_id: Some("task-root".to_string()),
                title: "Verify visible result".to_string(),
                objective: "Confirm tree, result, evidence and evaluation rendering.".to_string(),
                requirements: vec!["No duplicate lifecycle cards".to_string()],
                status: ProjectTaskStatus::Completed,
                executor: ProjectTaskExecutor::ProjectAgent {
                    agent_id: "query".to_string(),
                },
                execution_task_id: Some("worker-020".to_string()),
                session_thread_id: Some("01900000-0000-7000-8000-000000000020".to_string()),
                result: Some(ProjectTaskResult {
                    status: ProjectTaskResultStatus::Completed,
                    summary: "The task tree renders in one workspace.".to_string(),
                    artifacts: vec!["tui/task-workspace.snap".to_string()],
                    evidence: vec!["public JSON-RPC integration passed".to_string()],
                    suggested_children: vec![ProjectTaskSuggestedChild {
                        title: "Polish keyboard actions".to_string(),
                        objective: "Bind direct task operations.".to_string(),
                    }],
                    error: None,
                    completed_at: 18,
                }),
                evaluation: Some(ProjectTaskEvaluation {
                    verdict: ProjectTaskEvaluationVerdict::Passed,
                    summary: "The semantic node owns its final result.".to_string(),
                    evidence: vec!["snapshot reviewed".to_string()],
                    evaluated_at: 20,
                }),
                created_at: 11,
                updated_at: 20,
            },
        ],
    }
}

fn test_agents() -> Vec<ProjectAgentRosterEntry> {
    vec![ProjectAgentRosterEntry {
        id: "query".to_string(),
        description: "Focused repository query executor".to_string(),
        enabled: true,
        active_session_thread_id: None,
        session: None,
        current_task: None,
    }]
}
