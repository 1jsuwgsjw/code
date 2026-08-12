use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Weak;

use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_extension_api::ExtensionData;
use codex_extension_api::NoopExtensionEventSink;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::ToolVisibilityContributor;
use codex_extension_api::ToolVisibilityPolicy;
use codex_project_agents::LoadedProjectAgentTool;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentDefinition;
use codex_project_agents::ProjectAgentEntry;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentRuntime;
use codex_project_agents::ProjectAgentStore;
use codex_project_agents::ProjectAgentTaskResult;
use codex_project_agents::ProjectAgentTaskStatus;
use codex_project_agents::ProjectAgentToolManifest;
use codex_project_agents::ProjectAgentToolTarget;
use codex_project_agents::RelativeProjectAgentPath;
use codex_protocol::ThreadId;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_tools::ResponsesApiNamespaceTool;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Mutex;
use tokio::sync::RwLock;

use crate::delegate::parse_worker_result;
use crate::delegate::prepare_worker_config;
use crate::delegate::project_agent_result_schema;
use crate::delegate::worker_context_prompt;
use crate::events::ProjectAgentEventEmitter;
use crate::state::ProjectAgentExtension;
use crate::state::ProjectAgentRootContext;
use crate::state::ProjectAgentWorkerContext;
use crate::x_tools::project_agent_x_tools;
use crate::x_tools::worker_visible_tool_names;

#[derive(Debug, Eq, PartialEq)]
struct NamespacedToolSummary {
    tool_name: ToolName,
    namespace: String,
    function: String,
}

#[derive(Debug, Eq, PartialEq)]
struct WorkerIsolationSummary {
    include_permissions_instructions: bool,
    include_apps_instructions: bool,
    include_collaboration_mode_instructions: bool,
    include_skill_instructions: bool,
    orchestrator_skills_enabled: bool,
    orchestrator_mcp_enabled: bool,
    include_environment_context: bool,
    project_doc_max_bytes: usize,
    project_doc_fallback_filenames: Vec<String>,
    developer_instructions: Option<String>,
    notify_is_none: bool,
    ephemeral: bool,
    generate_memories: bool,
    use_memories: bool,
    dedicated_memory_tools: bool,
}

#[tokio::test]
async fn root_tools_skip_disabled_agents() {
    let project = TempDir::new().expect("project tempdir");
    let codex_home = TempDir::new().expect("codex home tempdir");
    let config = test_config(&codex_home, project.path()).await;
    let environment_manager = Arc::new(EnvironmentManager::default_for_tests());
    let file_system = environment_manager
        .get_environment(LOCAL_ENVIRONMENT_ID)
        .expect("local environment")
        .get_filesystem();
    let context = ProjectAgentRootContext {
        thread_id: ThreadId::new(),
        config,
        environments: Vec::new(),
        primary_environment_id: LOCAL_ENVIRONMENT_ID.to_string(),
        file_system,
        store: project_store(project.path()),
        enabled_agents: vec![agent_entry("enabled", true), agent_entry("disabled", false)],
        event_emitter: ProjectAgentEventEmitter::new(Arc::new(NoopExtensionEventSink)),
        task_gates: Arc::new(Mutex::new(BTreeMap::new())),
        active_sessions: Arc::new(RwLock::new(BTreeMap::new())),
        task_workspace_gate: Arc::new(Mutex::new(())),
    };
    let thread_store = ExtensionData::new("root-thread");
    thread_store.insert(context);
    let session_store = ExtensionData::new("session");
    let extension = ProjectAgentExtension::new(
        Weak::<ThreadManager>::new(),
        Arc::clone(&environment_manager),
        Arc::new(NoopExtensionEventSink),
    );

    let tools = extension.tools(&session_store, &thread_store);
    let names = tools
        .iter()
        .map(|tool| tool.tool_name())
        .collect::<Vec<_>>();

    assert_eq!(names, vec![ToolName::namespaced("agent", "enabled")]);
    assert_eq!(
        extension.visibility(&session_store, &thread_store),
        ToolVisibilityPolicy::default()
    );

    let context = thread_store
        .get::<ProjectAgentRootContext>()
        .expect("root context");
    let agent_id = agent_id("enabled");
    let first_gate = context.task_gate(&agent_id).await;
    let second_gate = context.task_gate(&agent_id).await;
    assert!(Arc::ptr_eq(&first_gate, &second_gate));
    assert_eq!(first_gate.available_permits(), 1);
    let worker_thread_id = ThreadId::new();
    context
        .remember_session(agent_id.clone(), worker_thread_id)
        .await;
    assert_eq!(
        context.active_session(&agent_id).await,
        Some(worker_thread_id)
    );
    context.forget_session(&agent_id, ThreadId::new()).await;
    assert_eq!(
        context.active_session(&agent_id).await,
        Some(worker_thread_id)
    );
    context.forget_session(&agent_id, worker_thread_id).await;
    assert_eq!(context.active_session(&agent_id).await, None);
}

#[tokio::test]
async fn worker_visibility_and_wrappers_follow_manifest_targets() {
    let project = TempDir::new().expect("project tempdir");
    let tools = vec![
        loaded_tool(
            "shell",
            ProjectAgentToolTarget::Native {
                tool: "shell_command".to_string(),
            },
        ),
        loaded_tool(
            "clock",
            ProjectAgentToolTarget::Native {
                tool: "clock.curr_time".to_string(),
            },
        ),
        loaded_tool(
            "run-script",
            ProjectAgentToolTarget::Command {
                program: relative_path("bin/run.py"),
            },
        ),
        loaded_tool(
            "lookup",
            ProjectAgentToolTarget::Mcp {
                server: "knowledge".to_string(),
                tool: "lookup".to_string(),
            },
        ),
    ];
    let context = Arc::new(worker_context(project.path(), tools.clone()));
    let expected_visible = vec![
        ToolName::plain("shell_command"),
        ToolName::namespaced("clock", "curr_time"),
        ToolName::namespaced("x", "run-script"),
        ToolName::namespaced("x", "lookup"),
    ];
    assert_eq!(worker_visible_tool_names(&tools), expected_visible);

    let environment_manager = Arc::new(EnvironmentManager::default_for_tests());
    let wrappers = project_agent_x_tools(
        Arc::clone(&context),
        Weak::<ThreadManager>::new(),
        Arc::clone(&environment_manager),
    );
    let summaries = wrappers
        .iter()
        .map(|tool| {
            let ToolSpec::Namespace(namespace) = tool.spec() else {
                panic!("project AGENT wrapper must be namespaced");
            };
            let [ResponsesApiNamespaceTool::Function(function)] = namespace.tools.as_slice() else {
                panic!("project AGENT wrapper must expose exactly one function");
            };
            NamespacedToolSummary {
                tool_name: tool.tool_name(),
                namespace: namespace.name,
                function: function.name.clone(),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summaries,
        vec![
            NamespacedToolSummary {
                tool_name: ToolName::namespaced("x", "run-script"),
                namespace: "x".to_string(),
                function: "run-script".to_string(),
            },
            NamespacedToolSummary {
                tool_name: ToolName::namespaced("x", "lookup"),
                namespace: "x".to_string(),
                function: "lookup".to_string(),
            },
        ]
    );

    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("worker-thread");
    thread_store.insert((*context).clone());
    let extension = ProjectAgentExtension::new(
        Weak::<ThreadManager>::new(),
        environment_manager,
        Arc::new(NoopExtensionEventSink),
    );
    assert_eq!(
        extension.visibility(&session_store, &thread_store),
        ToolVisibilityPolicy::allow_only(expected_visible)
    );
}

#[test]
fn worker_prompt_contains_runtime_contract_and_stays_bounded() {
    let project = TempDir::new().expect("project tempdir");
    let mut lookup = loaded_tool(
        "lookup",
        ProjectAgentToolTarget::Command {
            program: relative_path("tools/private_lookup.py"),
        },
    );
    lookup.input_schema = Some(json!({
        "type": "object",
        "properties": {"query": {"type": "string"}},
        "required": ["query"],
        "additionalProperties": false
    }));
    let mut context = worker_context(project.path(), vec![lookup]);
    context.runtime.constraints = "Stay inside the assigned repository scope.".to_string();
    context.runtime.accepted_memory = vec!["The owner is src/owner.rs.".to_string()];
    let prompt = worker_context_prompt(&context);
    for expected in [
        "Identity: query",
        "Each concrete task arrives as the current user turn.",
        "# Fixed result protocol",
        "Stay inside the assigned repository scope.",
        "lookup tool",
        "route: x.lookup",
        "input schema:",
        "\"query\"",
        "The owner is src/owner.rs.",
    ] {
        assert!(
            prompt.contains(expected),
            "missing prompt fragment: {expected}"
        );
    }
    assert!(!prompt.contains("task-1"));
    assert!(!prompt.contains("Inspect the target"));
    assert!(!prompt.contains("private_lookup.py"));

    context.runtime.constraints = "界".repeat(40_000);
    let bounded = worker_context_prompt(&context);
    assert!(bounded.len() <= 36 * 1024);
    assert!(bounded.is_char_boundary(bounded.len()));
}

#[test]
fn result_schema_requires_the_fixed_nine_field_contract() {
    let agent_id = agent_id("query");
    assert_eq!(
        project_agent_result_schema(&agent_id, Some("task-1")),
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": [
                        "completed",
                        "rejected_out_of_scope",
                        "blocked_missing_tool",
                        "failed"
                    ]
                },
                "agent_id": {"type": "string", "enum": ["query"]},
                "task_id": {"type": "string", "enum": ["task-1"]},
                "result": {"type": "string"},
                "artifacts": {"type": "array", "items": {"type": "string"}},
                "evidence": {"type": "array", "items": {"type": "string"}},
                "memory_candidates": {"type": "array", "items": {"type": "string"}},
                "improvement_proposals": {"type": "array", "items": {"type": "string"}},
                "error": {"type": ["string", "null"]}
            },
            "required": [
                "status",
                "agent_id",
                "task_id",
                "result",
                "artifacts",
                "evidence",
                "memory_candidates",
                "improvement_proposals",
                "error"
            ],
            "additionalProperties": false
        })
    );
}

#[tokio::test]
async fn worker_config_applies_overrides_and_disables_host_context() {
    let project = TempDir::new().expect("project tempdir");
    let codex_home = TempDir::new().expect("codex home tempdir");
    let mut config = test_config(&codex_home, project.path()).await;
    config.include_permissions_instructions = true;
    config.include_apps_instructions = true;
    config.include_collaboration_mode_instructions = true;
    config.include_skill_instructions = true;
    config.orchestrator_skills_enabled = true;
    config.orchestrator_mcp_enabled = true;
    config.include_environment_context = true;
    config.project_doc_max_bytes = 1234;
    config.project_doc_fallback_filenames = vec!["PROJECT.md".to_string()];
    config.developer_instructions = Some("host instructions".to_string());
    config.memories.generate_memories = true;
    config.memories.use_memories = true;
    config.memories.dedicated_tools = true;
    config
        .model_providers
        .insert("worker-provider".to_string(), config.model_provider.clone());

    let mut definition = agent_definition("query");
    definition.model = Some("worker-model".to_string());
    definition.model_reasoning_effort = Some("high".to_string());
    definition.model_provider = Some("worker-provider".to_string());
    let prepared = prepare_worker_config(config, &definition).expect("worker config");

    assert_eq!(prepared.model.as_deref(), Some("worker-model"));
    assert_eq!(prepared.model_reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(prepared.model_provider_id, "worker-provider");
    assert_eq!(
        WorkerIsolationSummary {
            include_permissions_instructions: prepared.include_permissions_instructions,
            include_apps_instructions: prepared.include_apps_instructions,
            include_collaboration_mode_instructions: prepared
                .include_collaboration_mode_instructions,
            include_skill_instructions: prepared.include_skill_instructions,
            orchestrator_skills_enabled: prepared.orchestrator_skills_enabled,
            orchestrator_mcp_enabled: prepared.orchestrator_mcp_enabled,
            include_environment_context: prepared.include_environment_context,
            project_doc_max_bytes: prepared.project_doc_max_bytes,
            project_doc_fallback_filenames: prepared.project_doc_fallback_filenames,
            developer_instructions: prepared.developer_instructions,
            notify_is_none: prepared.notify.is_none(),
            ephemeral: prepared.ephemeral,
            generate_memories: prepared.memories.generate_memories,
            use_memories: prepared.memories.use_memories,
            dedicated_memory_tools: prepared.memories.dedicated_tools,
        },
        WorkerIsolationSummary {
            include_permissions_instructions: false,
            include_apps_instructions: false,
            include_collaboration_mode_instructions: false,
            include_skill_instructions: false,
            orchestrator_skills_enabled: false,
            orchestrator_mcp_enabled: false,
            include_environment_context: false,
            project_doc_max_bytes: 0,
            project_doc_fallback_filenames: Vec::new(),
            developer_instructions: None,
            notify_is_none: true,
            ephemeral: false,
            generate_memories: false,
            use_memories: false,
            dedicated_memory_tools: false,
        }
    );
}

#[test]
fn invalid_worker_result_becomes_a_host_failure_without_candidates() {
    let result = parse_worker_result(
        &json!({
            "status": "completed",
            "agent_id": "other",
            "task_id": "task-1",
            "result": "untrusted",
            "artifacts": ["artifact"],
            "evidence": ["evidence"],
            "memory_candidates": ["candidate"],
            "improvement_proposals": ["proposal"],
            "error": null
        })
        .to_string(),
        &agent_id("query"),
        "task-1",
    );

    assert_eq!(
        result,
        ProjectAgentTaskResult {
            status: ProjectAgentTaskStatus::Failed,
            agent_id: agent_id("query"),
            task_id: "task-1".to_string(),
            result: "The project AGENT worker did not produce a valid completed result."
                .to_string(),
            artifacts: Vec::new(),
            evidence: Vec::new(),
            memory_candidates: Vec::new(),
            improvement_proposals: Vec::new(),
            error: Some(
                "invalid project AGENT result: invalid agent_id: result names `other`, expected `query`"
                    .to_string(),
            ),
        }
    );
}

#[tokio::test]
async fn lifecycle_bootstraps_root_and_stops_recursive_worker_bootstrap() {
    let project = TempDir::new().expect("project tempdir");
    std::fs::create_dir(project.path().join(".git")).expect("project marker");
    let codex_home = TempDir::new().expect("codex home tempdir");
    let config = test_config(&codex_home, project.path()).await;
    let environment_manager = Arc::new(EnvironmentManager::default_for_tests());
    let extension = ProjectAgentExtension::new(
        Weak::<ThreadManager>::new(),
        Arc::clone(&environment_manager),
        Arc::new(NoopExtensionEventSink),
    );
    let environments = vec![TurnEnvironmentSelection {
        environment_id: LOCAL_ENVIRONMENT_ID.to_string(),
        cwd: PathUri::from_host_native_path(project.path()).expect("project URI"),
    }];
    let session_source = SessionSource::Cli;
    let session_store = ExtensionData::new("session");
    let root_store = ExtensionData::new(ThreadId::new().to_string());

    extension
        .on_thread_start(ThreadStartInput {
            config: &config,
            session_source: &session_source,
            persistent_thread_state_available: true,
            environments: &environments,
            session_store: &session_store,
            thread_store: &root_store,
        })
        .await;

    let root_context = root_store
        .get::<ProjectAgentRootContext>()
        .expect("root context");
    assert!(root_context.enabled_agents.is_empty());
    assert!(project.path().join("AGENT/registry.toml").is_file());

    let worker_thread_id = ThreadId::new();
    let worker_store = ExtensionData::new(worker_thread_id.to_string());
    worker_store.insert(worker_context(project.path(), Vec::new()));
    extension
        .on_thread_start(ThreadStartInput {
            config: &config,
            session_source: &session_source,
            persistent_thread_state_available: false,
            environments: &environments,
            session_store: &session_store,
            thread_store: &worker_store,
        })
        .await;

    assert!(worker_store.get::<ProjectAgentRootContext>().is_none());
    assert_eq!(
        worker_store
            .get::<ProjectAgentWorkerContext>()
            .expect("worker context")
            .thread_id,
        Some(worker_thread_id)
    );
}

async fn test_config(codex_home: &TempDir, cwd: &Path) -> Config {
    ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .fallback_cwd(Some(cwd.to_path_buf()))
        .build()
        .await
        .expect("test config")
}

fn agent_id(value: &str) -> ProjectAgentId {
    ProjectAgentId::new(value).expect("agent id")
}

fn relative_path(value: &str) -> RelativeProjectAgentPath {
    RelativeProjectAgentPath::new(value).expect("relative project AGENT path")
}

fn agent_definition(id: &str) -> ProjectAgentDefinition {
    ProjectAgentDefinition {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        id: agent_id(id),
        description: format!("{id} specialist"),
        constraints_file: relative_path("constraints.md"),
        model: None,
        model_reasoning_effort: None,
        model_provider: None,
        tools: Vec::new(),
        memory_max_items: 16,
        memory_max_tokens: 2_000,
    }
}

fn agent_entry(id: &str, enabled: bool) -> ProjectAgentEntry {
    ProjectAgentEntry {
        path: relative_path(&format!("agents/{id}/agent.toml")),
        enabled,
        definition: agent_definition(id),
    }
}

fn loaded_tool(id: &str, target: ProjectAgentToolTarget) -> LoadedProjectAgentTool {
    LoadedProjectAgentTool {
        manifest_path: relative_path(&format!("tools/{id}.x")),
        manifest: ProjectAgentToolManifest {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: agent_id(id),
            description: format!("{id} tool"),
            target,
            timeout_ms: None,
            input_schema: None,
        },
        input_schema: None,
    }
}

fn project_store(project_root: &Path) -> ProjectAgentStore {
    ProjectAgentStore::new(PathUri::from_host_native_path(project_root).expect("project root URI"))
        .expect("project AGENT store")
}

fn worker_context(
    project_root: &Path,
    tools: Vec<LoadedProjectAgentTool>,
) -> ProjectAgentWorkerContext {
    ProjectAgentWorkerContext {
        thread_id: None,
        store: project_store(project_root),
        runtime: ProjectAgentRuntime {
            entry: agent_entry("query", true),
            constraints: "Keep the task bounded.".to_string(),
            tools,
            accepted_memory: Vec::new(),
        },
        primary_environment_id: LOCAL_ENVIRONMENT_ID.to_string(),
    }
}
