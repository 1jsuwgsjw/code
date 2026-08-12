use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn semantic_task_workspace_round_trips_tree_execution_result_and_evaluation() {
    let temp_dir = tempdir().expect("tempdir");
    let project_root = AbsolutePathBuf::try_from(temp_dir.path()).expect("absolute temp path");
    let store = ProjectAgentStore::new(PathUri::from_abs_path(&project_root)).expect("store");
    let file_system = super::tests::TestFileSystem;
    let scope = ProjectAgentFileSystemScope::Unrestricted;
    let root_id = ProjectTaskId::new("improve-agent-tui").expect("root task id");
    let child_id = ProjectTaskId::new("inspect-current-ui").expect("child task id");
    let mut workspace = ProjectTaskWorkspace::default();

    workspace
        .create_task(
            root_id.clone(),
            None,
            "改进 AGENT TUI".to_string(),
            "交付围绕语义任务组织的任务工作台。".to_string(),
            ProjectTaskExecutor::MainAgent,
            10,
        )
        .expect("create root task");
    workspace
        .create_task(
            child_id.clone(),
            Some(root_id.clone()),
            "核对当前实现".to_string(),
            "找出当前面板割裂的具体原因。".to_string(),
            ProjectTaskExecutor::Unassigned,
            11,
        )
        .expect("create child task");
    workspace
        .append_requirement(&child_id, "保留旧 rollout 的读取兼容。".to_string(), 12)
        .expect("append requirement");
    workspace
        .start_execution(
            &child_id,
            ProjectAgentId::new("query").expect("agent id"),
            "019ff546-3156-7cb3-98c5-e2d796c33852".to_string(),
            13,
        )
        .expect("start execution");
    workspace
        .record_result(
            &child_id,
            ProjectTaskResult {
                status: ProjectTaskResultStatus::Completed,
                summary: "当前面板把一个任务拆成多张生命周期通知卡。".to_string(),
                artifacts: vec!["task/artifacts/ui-review.md".to_string()],
                evidence: vec!["开始和完成使用两张独立历史卡。".to_string()],
                suggested_children: vec![ProjectTaskSuggestedChild {
                    title: "改为原地更新".to_string(),
                    objective: "让运行中的任务卡原地进入完成状态。".to_string(),
                }],
                error: None,
                completed_at: 14,
            },
        )
        .expect("record result");
    workspace
        .set_evaluation(
            &child_id,
            ProjectTaskEvaluation {
                verdict: ProjectTaskEvaluationVerdict::Passed,
                summary: "目标符合，证据充分。".to_string(),
                evidence: vec!["定位到了重复生命周期卡的写入点。".to_string()],
                evaluated_at: 15,
            },
        )
        .expect("set evaluation");

    let path = store
        .persist_task_workspace(&file_system, scope, &workspace)
        .await
        .expect("persist task workspace");
    let restored = store
        .task_workspace(&file_system, scope)
        .await
        .expect("load task workspace");

    assert_eq!(
        path,
        PathUri::from_abs_path(&project_root.join("AGENT/tasks/workspace.json"))
    );
    assert_eq!(restored, workspace);
    assert_eq!(
        restored.root_tasks().cloned().collect::<Vec<_>>(),
        vec![restored.task(&root_id).expect("root task").clone()]
    );
    assert_eq!(
        restored.child_tasks(&root_id).cloned().collect::<Vec<_>>(),
        vec![restored.task(&child_id).expect("child task").clone()]
    );
}

#[test]
fn semantic_task_workspace_rejects_duplicate_or_terminal_mutation() {
    let root_id = ProjectTaskId::new("root").expect("root task id");
    let mut workspace = ProjectTaskWorkspace::default();
    workspace
        .create_task(
            root_id.clone(),
            None,
            "Root".to_string(),
            "Complete the root task.".to_string(),
            ProjectTaskExecutor::MainAgent,
            1,
        )
        .expect("create root task");

    assert_eq!(
        workspace
            .create_task(
                root_id.clone(),
                None,
                "Duplicate".to_string(),
                "Must be rejected.".to_string(),
                ProjectTaskExecutor::Unassigned,
                2,
            )
            .expect_err("duplicate task must fail"),
        ProjectTaskWorkspaceError::TaskAlreadyExists(root_id.clone())
    );

    workspace
        .record_result(
            &root_id,
            ProjectTaskResult {
                status: ProjectTaskResultStatus::Completed,
                summary: "Done".to_string(),
                artifacts: Vec::new(),
                evidence: Vec::new(),
                suggested_children: Vec::new(),
                error: None,
                completed_at: 3,
            },
        )
        .expect("record result");
    assert_eq!(
        workspace
            .append_requirement(&root_id, "Late requirement".to_string(), 4)
            .expect_err("terminal mutation must fail"),
        ProjectTaskWorkspaceError::InvalidState {
            task_id: root_id,
            status: ProjectTaskStatus::Completed,
            operation: "append a requirement",
        }
    );
}
