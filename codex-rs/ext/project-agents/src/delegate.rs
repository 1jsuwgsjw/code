use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use codex_core::StartThreadOptions;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_project_agents::ProjectAgentDefinition;
use codex_project_agents::ProjectAgentEntry;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentTaskResult;
use codex_project_agents::ProjectAgentTaskStatus;
use codex_project_agents::ProjectAgentToolTarget;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_utils_string::take_bytes_at_char_boundary;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

use crate::state::ProjectAgentRootContext;
use crate::state::ProjectAgentWorkerContext;

pub(crate) const AGENT_NAMESPACE: &str = "agent";
const MAX_TASK_BYTES: usize = 16 * 1024;
const MAX_WORKER_PROMPT_BYTES: usize = 36 * 1024;
const MAX_ERROR_BYTES: usize = 8 * 1024;
const WORKER_TURN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Deserialize)]
struct DelegateArgs {
    task: String,
}

pub(crate) struct ProjectAgentDelegateTool {
    context: Arc<ProjectAgentRootContext>,
    entry: ProjectAgentEntry,
    thread_manager: Weak<ThreadManager>,
}

impl ProjectAgentDelegateTool {
    pub(crate) fn new(
        context: Arc<ProjectAgentRootContext>,
        entry: ProjectAgentEntry,
        thread_manager: Weak<ThreadManager>,
    ) -> Self {
        Self {
            context,
            entry,
            thread_manager,
        }
    }
}

impl ToolExecutor<ToolCall> for ProjectAgentDelegateTool {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(AGENT_NAMESPACE, self.entry.definition.id.as_str())
    }

    fn spec(&self) -> ToolSpec {
        let mut properties = BTreeMap::new();
        properties.insert(
            "task".to_string(),
            JsonSchema::string(Some(
                "One bounded unit of work for this specialist AGENT.".to_string(),
            )),
        );
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: AGENT_NAMESPACE.to_string(),
            description: "Project-local specialist AGENTs.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: self.entry.definition.id.to_string(),
                description: self.entry.definition.description.clone(),
                strict: true,
                defer_loading: None,
                parameters: JsonSchema::object(
                    properties,
                    Some(vec!["task".to_string()]),
                    Some(false.into()),
                ),
                output_schema: Some(project_agent_result_schema(
                    &self.entry.definition.id,
                    /*task_id*/ None,
                )),
            })],
        })
    }

    fn handle(&self, invocation: ToolCall) -> ToolExecutorFuture<'_> {
        let context = Arc::clone(&self.context);
        let agent_id = self.entry.definition.id.clone();
        let thread_manager = self.thread_manager.clone();
        Box::pin(async move {
            let arguments = invocation.function_arguments()?;
            let DelegateArgs { task } = serde_json::from_str(arguments).map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "invalid project AGENT task arguments: {error}"
                ))
            })?;
            if task.trim().is_empty() || task.len() > MAX_TASK_BYTES {
                return Err(FunctionCallError::RespondToModel(format!(
                    "project AGENT task must be non-empty and at most {MAX_TASK_BYTES} bytes"
                )));
            }

            let task_id = Uuid::now_v7().to_string();
            let selected_environment = invocation
                .environments
                .iter()
                .find(|environment| environment.environment_id == context.primary_environment_id)
                .cloned()
                .or_else(|| invocation.environments.first().cloned());
            let file_system = selected_environment
                .as_ref()
                .map(|environment| Arc::clone(&environment.file_system))
                .unwrap_or_else(|| Arc::clone(&context.file_system));
            let scope = selected_environment
                .as_ref()
                .map(|environment| {
                    ProjectAgentFileSystemScope::Sandboxed(&environment.file_system_sandbox_context)
                })
                .unwrap_or(ProjectAgentFileSystemScope::Unrestricted);

            let result = match thread_manager.upgrade() {
                Some(thread_manager) => {
                    run_worker(
                        thread_manager,
                        Arc::clone(&context),
                        agent_id.clone(),
                        task_id.clone(),
                        task,
                        file_system.as_ref(),
                        scope,
                    )
                    .await
                }
                None => host_failed_result(
                    &agent_id,
                    &task_id,
                    "project AGENT thread manager is unavailable",
                ),
            };
            let result = match context
                .store
                .persist_result(file_system.as_ref(), scope, &result)
                .await
            {
                Ok(_) => {
                    context.emit_maintenance_status().await;
                    result
                }
                Err(error) => host_failed_result(
                    &agent_id,
                    &task_id,
                    &format!("failed to persist project AGENT result: {error}"),
                ),
            };
            let value = serde_json::to_value(result).map_err(|error| {
                FunctionCallError::Fatal(format!(
                    "failed to serialize project AGENT result: {error}"
                ))
            })?;
            let output: Box<dyn ToolOutput> = Box::new(JsonToolOutput::new(value));
            Ok(output)
        })
    }
}

async fn run_worker(
    thread_manager: Arc<ThreadManager>,
    root_context: Arc<ProjectAgentRootContext>,
    agent_id: ProjectAgentId,
    task_id: String,
    task: String,
    file_system: &dyn codex_exec_server::ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
) -> ProjectAgentTaskResult {
    let runtime = match root_context
        .store
        .load_runtime(file_system, scope, &agent_id)
        .await
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return host_failed_result(
                &agent_id,
                &task_id,
                &format!("failed to load project AGENT runtime: {error}"),
            );
        }
    };
    let config = match prepare_worker_config(root_context.config.clone(), &runtime.entry.definition)
    {
        Ok(config) => config,
        Err(error) => return host_failed_result(&agent_id, &task_id, &error),
    };
    let worker_context = ProjectAgentWorkerContext {
        thread_id: None,
        store: root_context.store.clone(),
        runtime,
        task_id: task_id.clone(),
        task: task.clone(),
        primary_environment_id: root_context.primary_environment_id.clone(),
    };
    let mut thread_extension_init = ExtensionDataInit::new();
    thread_extension_init.insert(worker_context);
    let new_thread = match thread_manager
        .start_thread_with_options(StartThreadOptions {
            config,
            allow_provider_model_fallback: false,
            initial_history: InitialHistory::New,
            history_mode: None,
            session_source: None,
            thread_source: None,
            dynamic_tools: Vec::new(),
            metrics_service_name: None,
            parent_trace: None,
            environments: root_context.environments.clone(),
            thread_extension_init,
            supports_openai_form_elicitation: false,
        })
        .await
    {
        Ok(new_thread) => new_thread,
        Err(error) => {
            return host_failed_result(
                &agent_id,
                &task_id,
                &format!("failed to start project AGENT worker: {error}"),
            );
        }
    };

    let worker_result = tokio::time::timeout(WORKER_TURN_TIMEOUT, async {
        new_thread
            .thread
            .submit(Op::UserInput {
                items: vec![UserInput::Text {
                    text: task,
                    text_elements: Vec::new(),
                }],
                final_output_json_schema: Some(project_agent_result_schema(
                    &agent_id,
                    Some(&task_id),
                )),
                responsesapi_client_metadata: None,
                additional_context: Default::default(),
                thread_settings: Default::default(),
            })
            .await
            .map_err(|error| format!("failed to submit project AGENT task: {error}"))?;
        loop {
            let event = new_thread
                .thread
                .next_event()
                .await
                .map_err(|error| format!("failed to read project AGENT event: {error}"))?;
            if let EventMsg::TurnComplete(completed) = event.msg {
                return completed
                    .last_agent_message
                    .ok_or_else(|| "project AGENT worker returned no final message".to_string());
            }
        }
    })
    .await;
    let _ = new_thread.thread.shutdown_and_wait().await;
    thread_manager.remove_thread(&new_thread.thread_id).await;

    match worker_result {
        Ok(Ok(raw_result)) => parse_worker_result(&raw_result, &agent_id, &task_id),
        Ok(Err(error)) => host_failed_result(&agent_id, &task_id, &error),
        Err(_) => host_failed_result(
            &agent_id,
            &task_id,
            "project AGENT worker exceeded the 30 minute turn limit",
        ),
    }
}

pub(crate) fn prepare_worker_config(
    mut config: Config,
    definition: &ProjectAgentDefinition,
) -> Result<Config, String> {
    if let Some(model) = &definition.model {
        config.model = Some(model.clone());
    }
    if let Some(effort) = &definition.model_reasoning_effort {
        config.model_reasoning_effort = Some(
            effort
                .parse::<ReasoningEffort>()
                .map_err(|error| format!("invalid model reasoning effort `{effort}`: {error}"))?,
        );
    }
    if let Some(provider_id) = definition
        .model_provider
        .as_deref()
        .filter(|provider_id| *provider_id != "default")
    {
        let provider = config
            .model_providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| format!("model provider `{provider_id}` is not configured"))?;
        config.model_provider_id = provider_id.to_string();
        config.model_provider = provider;
    }

    config.include_permissions_instructions = false;
    config.include_apps_instructions = false;
    config.include_collaboration_mode_instructions = false;
    config.include_skill_instructions = false;
    config.orchestrator_skills_enabled = false;
    config.orchestrator_mcp_enabled = false;
    config.include_environment_context = false;
    config.project_doc_max_bytes = 0;
    config.project_doc_fallback_filenames.clear();
    config.developer_instructions = None;
    config.notify = None;
    config.ephemeral = true;
    config.memories.generate_memories = false;
    config.memories.use_memories = false;
    config.memories.dedicated_tools = false;
    Ok(config)
}

pub(crate) fn parse_worker_result(
    raw_result: &str,
    agent_id: &ProjectAgentId,
    task_id: &str,
) -> ProjectAgentTaskResult {
    let result = serde_json::from_str::<ProjectAgentTaskResult>(raw_result)
        .map_err(|error| format!("invalid project AGENT result JSON: {error}"))
        .and_then(|result| {
            result
                .validate_for(agent_id, task_id)
                .map(|()| result)
                .map_err(|error| format!("invalid project AGENT result: {error}"))
        });
    result.unwrap_or_else(|error| host_failed_result(agent_id, task_id, &error))
}

fn host_failed_result(
    agent_id: &ProjectAgentId,
    task_id: &str,
    error: &str,
) -> ProjectAgentTaskResult {
    let error = take_bytes_at_char_boundary(error.trim(), MAX_ERROR_BYTES);
    ProjectAgentTaskResult {
        status: ProjectAgentTaskStatus::Failed,
        agent_id: agent_id.clone(),
        task_id: task_id.to_string(),
        result: "The project AGENT worker did not produce a valid completed result.".to_string(),
        artifacts: Vec::new(),
        evidence: Vec::new(),
        memory_candidates: Vec::new(),
        improvement_proposals: Vec::new(),
        error: Some(if error.is_empty() {
            "unknown project AGENT worker failure".to_string()
        } else {
            error.to_string()
        }),
    }
}

pub(crate) fn project_agent_result_schema(
    agent_id: &ProjectAgentId,
    task_id: Option<&str>,
) -> Value {
    let task_id_schema = task_id
        .map(|task_id| json!({"type": "string", "enum": [task_id]}))
        .unwrap_or_else(|| json!({"type": "string"}));
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
            "agent_id": {"type": "string", "enum": [agent_id.as_str()]},
            "task_id": task_id_schema,
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
}

pub(crate) fn worker_context_prompt(context: &ProjectAgentWorkerContext) -> String {
    let definition = &context.runtime.entry.definition;
    let mut prompt = String::new();
    append_prompt(
        &mut prompt,
        &format!(
            "# Project specialist AGENT\n\nIdentity: {}\nDescription: {}\nTask ID: {}\nCurrent task:\n{}\n\n",
            definition.id, definition.description, context.task_id, context.task
        ),
    );
    append_prompt(
        &mut prompt,
        "# Fixed result protocol\n\nReturn exactly one JSON object. Every top-level field is required: status, agent_id, task_id, result, artifacts, evidence, memory_candidates, improvement_proposals, and error. status must be completed, rejected_out_of_scope, blocked_missing_tool, or failed. agent_id and task_id must exactly match the values above. error must be null unless status is failed. Do not wrap the JSON in Markdown.\n\n",
    );
    append_prompt(
        &mut prompt,
        &format!("# Constraints\n\n{}\n\n", context.runtime.constraints),
    );
    append_prompt(&mut prompt, "# Registered tools\n\n");
    for tool in &context.runtime.tools {
        let route = match &tool.manifest.target {
            ProjectAgentToolTarget::Native { tool } => tool.clone(),
            ProjectAgentToolTarget::Command { .. } | ProjectAgentToolTarget::Mcp { .. } => {
                format!("x.{}", tool.manifest.id)
            }
        };
        append_prompt(
            &mut prompt,
            &format!(
                "- {}: {} (route: {})\n",
                tool.manifest.id, tool.manifest.description, route
            ),
        );
        if let Some(schema) = &tool.input_schema {
            append_prompt(&mut prompt, &format!("  input schema: {}\n", schema));
        }
    }
    append_prompt(&mut prompt, "\n# Accepted memory\n\n");
    for memory in &context.runtime.accepted_memory {
        append_prompt(&mut prompt, &format!("- {memory}\n"));
    }
    prompt
}

fn append_prompt(prompt: &mut String, text: &str) {
    let remaining = MAX_WORKER_PROMPT_BYTES.saturating_sub(prompt.len());
    if remaining == 0 {
        return;
    }
    prompt.push_str(take_bytes_at_char_boundary(text, remaining));
}
