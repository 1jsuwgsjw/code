use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_extension_api::NoopExtensionEventSink;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentStore;
use codex_project_agents::ProjectTaskEvaluationVerdict;
use codex_project_agents::ProjectTaskStatus;
use codex_protocol::ThreadId;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use super::*;
use crate::events::ProjectAgentEventEmitter;

#[tokio::test]
async fn runtime_workspace_mutations_preserve_one_semantic_task_tree() {
    let fixture = TaskWorkspaceFixture::new().await;
    let root = create_task(
        fixture.context.as_ref(),
        None,
        "Improve the AGENT workbench".to_string(),
        "Organize project AGENT work around semantic tasks.".to_string(),
        ProjectTaskExecutor::MainAgent,
    )
    .await
    .expect("create root task");
    let child = create_task(
        fixture.context.as_ref(),
        Some(root.task_id.clone()),
        "Inspect the current panel".to_string(),
        "Identify why execution lifecycle cards appear fragmented.".to_string(),
        ProjectTaskExecutor::Unassigned,
    )
    .await
    .expect("create child task");
    let child = append_requirement(
        fixture.context.as_ref(),
        &child.task_id,
        "Keep legacy rollout reads compatible.".to_string(),
    )
    .await
    .expect("append requirement");
    let result = ProjectTaskResult {
        status: ProjectTaskResultStatus::Completed,
        summary: "The current UI appends one card per lifecycle transition.".to_string(),
        artifacts: vec!["task/artifacts/ui-review.md".to_string()],
        evidence: vec!["Started and completed events render independently.".to_string()],
        suggested_children: vec![ProjectTaskSuggestedChild {
            title: "Render in place".to_string(),
            objective: "Update the selected task card without adding a sibling card.".to_string(),
        }],
        error: None,
        completed_at: child.updated_at,
    };
    let completed = record_result(fixture.context.as_ref(), &child.task_id, result.clone())
        .await
        .expect("record result");
    let evaluation = ProjectTaskEvaluation {
        verdict: ProjectTaskEvaluationVerdict::Passed,
        summary: "The finding is specific and supported.".to_string(),
        evidence: vec!["The writer and rendered cards were identified.".to_string()],
        evaluated_at: completed.updated_at,
    };
    let evaluated = set_evaluation(fixture.context.as_ref(), &child.task_id, evaluation.clone())
        .await
        .expect("set evaluation");

    let workspace = read_workspace(fixture.context.as_ref())
        .await
        .expect("read workspace");
    assert_eq!(
        workspace.root_tasks().cloned().collect::<Vec<_>>(),
        vec![root]
    );
    assert_eq!(
        workspace
            .child_tasks(
                evaluated
                    .parent_task_id
                    .as_ref()
                    .expect("child parent task id"),
            )
            .cloned()
            .collect::<Vec<_>>(),
        vec![evaluated.clone()]
    );
    assert_eq!(
        evaluated.requirements,
        vec!["Keep legacy rollout reads compatible.".to_string()]
    );
    assert_eq!(evaluated.result, Some(result));
    assert_eq!(evaluated.evaluation, Some(evaluation));
}

#[tokio::test]
async fn execution_binding_keeps_worker_lifecycle_inside_one_semantic_task() {
    let fixture = TaskWorkspaceFixture::new().await;
    let agent_id = ProjectAgentId::new("query").expect("agent id");
    fixture
        .context
        .store
        .create(
            fixture.context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            agent_id.clone(),
            "Query specialist".to_string(),
        )
        .await
        .expect("create project AGENT");
    let task = create_task(
        fixture.context.as_ref(),
        None,
        "Inspect the panel".to_string(),
        "Return one bounded finding.".to_string(),
        ProjectTaskExecutor::ProjectAgent {
            agent_id: agent_id.clone(),
        },
    )
    .await
    .expect("create semantic task");
    append_requirement(
        fixture.context.as_ref(),
        &task.task_id,
        "Include concrete evidence.".to_string(),
    )
    .await
    .expect("append requirement");

    let outcome = start_execution(None, Arc::clone(&fixture.context), &task.task_id, &agent_id)
        .await
        .expect("start execution");

    assert_ne!(outcome.task.task_id.as_str(), outcome.execution_task_id);
    assert_eq!(outcome.task.status, ProjectTaskStatus::InProgress);
    assert_eq!(
        outcome.task.execution_task_id.as_deref(),
        Some(outcome.execution_task_id.as_str())
    );
    let execution = fixture
        .context
        .store
        .task_metadata(
            fixture.context.file_system.as_ref(),
            ProjectAgentFileSystemScope::Unrestricted,
            &agent_id,
            &outcome.execution_task_id,
        )
        .await
        .expect("read execution metadata")
        .expect("execution metadata");
    assert!(execution.task.contains("Include concrete evidence."));

    let completed = wait_for_terminal_task(fixture.context.as_ref(), &task.task_id).await;
    assert_eq!(completed.task_id, task.task_id);
    assert_eq!(
        completed.execution_task_id,
        Some(outcome.execution_task_id.clone())
    );
    assert_eq!(completed.status, ProjectTaskStatus::Failed);
    assert_eq!(
        completed
            .result
            .as_ref()
            .expect("worker result written back")
            .status,
        ProjectTaskResultStatus::Failed
    );
    let workspace = read_workspace(fixture.context.as_ref())
        .await
        .expect("read workspace");
    assert_eq!(workspace.tasks, vec![completed]);
}

async fn wait_for_terminal_task(
    context: &ProjectAgentRootContext,
    task_id: &ProjectTaskId,
) -> ProjectTaskNode {
    for _ in 0..100 {
        let workspace = read_workspace(context).await.expect("read workspace");
        let task = workspace.task(task_id).expect("semantic task").clone();
        if !matches!(
            task.status,
            ProjectTaskStatus::Pending | ProjectTaskStatus::InProgress
        ) {
            return task;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("semantic task did not reach a terminal state");
}

struct TaskWorkspaceFixture {
    _project: TempDir,
    _codex_home: TempDir,
    context: Arc<ProjectAgentRootContext>,
}

impl TaskWorkspaceFixture {
    async fn new() -> Self {
        let project = TempDir::new().expect("project tempdir");
        std::fs::create_dir(project.path().join(".git")).expect("project marker");
        let codex_home = TempDir::new().expect("codex home tempdir");
        let config = ConfigBuilder::default()
            .codex_home(codex_home.path().to_path_buf())
            .fallback_cwd(Some(project.path().to_path_buf()))
            .build()
            .await
            .expect("test config");
        let environment_manager = EnvironmentManager::default_for_tests();
        let file_system = environment_manager
            .get_environment(LOCAL_ENVIRONMENT_ID)
            .expect("local environment")
            .get_filesystem();
        let store = project_store(project.path());
        store
            .bootstrap(
                file_system.as_ref(),
                ProjectAgentFileSystemScope::Unrestricted,
            )
            .await
            .expect("bootstrap project AGENT store");
        let context = Arc::new(ProjectAgentRootContext {
            thread_id: ThreadId::new(),
            config,
            environments: Vec::new(),
            primary_environment_id: LOCAL_ENVIRONMENT_ID.to_string(),
            file_system,
            store,
            enabled_agents: Vec::new(),
            event_emitter: ProjectAgentEventEmitter::new(Arc::new(NoopExtensionEventSink)),
            task_gates: Arc::new(Mutex::new(BTreeMap::new())),
            active_sessions: Arc::new(RwLock::new(BTreeMap::new())),
            task_workspace_gate: Arc::new(Mutex::new(())),
        });
        Self {
            _project: project,
            _codex_home: codex_home,
            context,
        }
    }
}

fn project_store(project_root: &Path) -> ProjectAgentStore {
    ProjectAgentStore::new(PathUri::from_host_native_path(project_root).expect("project root URI"))
        .expect("project AGENT store")
}
