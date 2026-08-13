use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Weak;
use std::time::Duration;

use codex_core::ThreadManager;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecOutputStream;
use codex_exec_server::ExecParams;
use codex_exec_server::ExecProcess;
use codex_exec_server::ProcessId;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolExecutorFuture;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_project_agents::LoadedProjectAgentTool;
use codex_project_agents::ProjectAgentToolTarget;
use codex_project_agents::RelativeProjectAgentPath;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ResponsesApiTool;
use codex_tools::parse_tool_input_schema_without_compaction;
use serde_json::Value;
use serde_json::json;
use tokio::time::Instant;
use uuid::Uuid;

use crate::state::ProjectAgentWorkerContext;

const X_NAMESPACE: &str = "x";
const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 60_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 300_000;
const MAX_COMMAND_OUTPUT_BYTES: usize = 256 * 1024;
const COMMAND_READ_WAIT_MS: u64 = 1_000;

pub(crate) fn project_agent_x_tools(
    context: Arc<ProjectAgentWorkerContext>,
    thread_manager: Weak<ThreadManager>,
    environment_manager: Arc<EnvironmentManager>,
) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
    context
        .runtime
        .tools
        .iter()
        .filter(|tool| !matches!(&tool.manifest.target, ProjectAgentToolTarget::Native { .. }))
        .cloned()
        .map(|tool| {
            Arc::new(ProjectAgentXTool {
                context: Arc::clone(&context),
                tool,
                thread_manager: thread_manager.clone(),
                environment_manager: Arc::clone(&environment_manager),
            }) as Arc<dyn ToolExecutor<ToolCall>>
        })
        .collect()
}

pub(crate) fn worker_visible_tool_names(tools: &[LoadedProjectAgentTool]) -> Vec<ToolName> {
    tools
        .iter()
        .map(|tool| match &tool.manifest.target {
            ProjectAgentToolTarget::Native { tool } => native_tool_name(tool),
            ProjectAgentToolTarget::Command { .. } | ProjectAgentToolTarget::Mcp { .. } => {
                ToolName::namespaced(X_NAMESPACE, tool.manifest.id.as_str())
            }
        })
        .collect()
}

fn native_tool_name(value: &str) -> ToolName {
    match value.split_once('.') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            ToolName::namespaced(namespace, name)
        }
        _ => ToolName::plain(value),
    }
}

struct ProjectAgentXTool {
    context: Arc<ProjectAgentWorkerContext>,
    tool: LoadedProjectAgentTool,
    thread_manager: Weak<ThreadManager>,
    environment_manager: Arc<EnvironmentManager>,
}

impl ToolExecutor<ToolCall> for ProjectAgentXTool {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(X_NAMESPACE, self.tool.manifest.id.as_str())
    }

    fn spec(&self) -> ToolSpec {
        let parameters = self
            .tool
            .input_schema
            .as_ref()
            .and_then(
                |schema| match parse_tool_input_schema_without_compaction(schema) {
                    Ok(schema) => Some(schema),
                    Err(error) => {
                        tracing::warn!(
                            tool = %self.tool.manifest.id,
                            %error,
                            "project AGENT tool input schema is not model-compatible"
                        );
                        None
                    }
                },
            )
            .unwrap_or_else(|| {
                JsonSchema::object(
                    BTreeMap::new(),
                    /*required*/ None,
                    /*additional_properties*/ Some(true.into()),
                )
            });
        ToolSpec::Namespace(ResponsesApiNamespace {
            name: X_NAMESPACE.to_string(),
            description: "Explicitly registered project AGENT tools.".to_string(),
            tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
                name: self.tool.manifest.id.to_string(),
                description: self.tool.manifest.description.clone(),
                strict: false,
                defer_loading: None,
                parameters,
                output_schema: None,
            })],
        })
    }

    fn handle(&self, invocation: ToolCall) -> ToolExecutorFuture<'_> {
        let context = Arc::clone(&self.context);
        let tool = self.tool.clone();
        let thread_manager = self.thread_manager.clone();
        let environment_manager = Arc::clone(&self.environment_manager);
        Box::pin(async move {
            let arguments = invocation.function_arguments()?;
            let arguments: Value = serde_json::from_str(arguments).map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "invalid project AGENT tool arguments: {error}"
                ))
            })?;
            if !arguments.is_object() {
                return Err(FunctionCallError::RespondToModel(
                    "project AGENT tool arguments must be a JSON object".to_string(),
                ));
            }
            match &tool.manifest.target {
                ProjectAgentToolTarget::Command { program } => {
                    run_command_tool(
                        &context,
                        &tool,
                        program,
                        arguments,
                        &invocation,
                        environment_manager.as_ref(),
                    )
                    .await
                }
                ProjectAgentToolTarget::Mcp { server, tool } => {
                    let thread_id = context.thread_id.ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "project AGENT worker thread is not initialized".to_string(),
                        )
                    })?;
                    let thread_manager = thread_manager.upgrade().ok_or_else(|| {
                        FunctionCallError::RespondToModel(
                            "project AGENT thread manager is unavailable".to_string(),
                        )
                    })?;
                    let thread = thread_manager
                        .get_thread(thread_id)
                        .await
                        .map_err(|error| {
                            FunctionCallError::RespondToModel(format!(
                                "project AGENT worker thread is unavailable: {error}"
                            ))
                        })?;
                    let result = thread
                        .call_mcp_tool(server, tool, Some(arguments), /*meta*/ None)
                        .await
                        .map_err(|error| {
                            FunctionCallError::RespondToModel(format!(
                                "project AGENT MCP tool failed: {error}"
                            ))
                        })?;
                    let success = !result.is_error.unwrap_or(false);
                    let result = serde_json::to_value(result).map_err(|error| {
                        FunctionCallError::Fatal(format!(
                            "failed to serialize project AGENT MCP result: {error}"
                        ))
                    })?;
                    let output: Box<dyn ToolOutput> =
                        Box::new(JsonToolOutput::with_success(result, Some(success)));
                    Ok(output)
                }
                ProjectAgentToolTarget::Native { .. } => Err(FunctionCallError::Fatal(
                    "native project AGENT tools must be dispatched by their host runtime"
                        .to_string(),
                )),
            }
        })
    }
}

async fn run_command_tool(
    context: &ProjectAgentWorkerContext,
    loaded_tool: &LoadedProjectAgentTool,
    program: &RelativeProjectAgentPath,
    arguments: Value,
    invocation: &ToolCall,
    environment_manager: &EnvironmentManager,
) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
    let environment = environment_manager
        .get_environment(&context.primary_environment_id)
        .ok_or_else(|| {
            FunctionCallError::RespondToModel(format!(
                "project AGENT environment `{}` is unavailable",
                context.primary_environment_id
            ))
        })?;
    let agent_id = &context.runtime.entry.definition.id;
    let agent_directory = RelativeProjectAgentPath::new(format!("agents/{agent_id}"))
        .map_err(|error| FunctionCallError::Fatal(error.to_string()))?;
    let program = RelativeProjectAgentPath::new(format!("agents/{agent_id}/{}", program.as_str()))
        .map_err(|error| FunctionCallError::Fatal(error.to_string()))?;
    let cwd = context
        .store
        .resolve(&agent_directory)
        .map_err(|error| FunctionCallError::Fatal(error.to_string()))?;
    let program = context
        .store
        .resolve(&program)
        .map_err(|error| FunctionCallError::Fatal(error.to_string()))?;
    let program_string = program.inferred_native_path_string();
    let arguments = serde_json::to_string(&arguments).map_err(|error| {
        FunctionCallError::Fatal(format!(
            "failed to serialize project AGENT tool arguments: {error}"
        ))
    })?;
    let argv = if program_string.to_ascii_lowercase().ends_with(".py") {
        vec!["python".to_string(), program_string, arguments]
    } else if program_string.to_ascii_lowercase().ends_with(".ps1") {
        vec!["powershell".to_string(), program_string, arguments]
    } else {
        vec![program_string, arguments]
    };
    #[cfg(windows)]
    let sandbox = None;
    #[cfg(not(windows))]
    let sandbox = invocation
        .environments
        .iter()
        .find(|candidate| candidate.environment_id == context.primary_environment_id)
        .or_else(|| invocation.environments.first())
        .map(|environment| environment.file_system_sandbox_context.clone());
    let timeout_ms = loaded_tool
        .manifest
        .timeout_ms
        .unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS)
        .min(MAX_COMMAND_TIMEOUT_MS);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let started = tokio::time::timeout_at(
        deadline,
        environment.get_exec_backend().start(ExecParams {
            process_id: ProcessId::new(format!("project-agent-{}", Uuid::now_v7())),
            argv,
            cwd,
            env_policy: None,
            env: HashMap::new(),
            tty: false,
            pipe_stdin: false,
            arg0: None,
            sandbox,
            enforce_managed_network: false,
            managed_network: None,
        }),
    )
    .await
    .map_err(|_| {
        FunctionCallError::RespondToModel(format!(
            "project AGENT command exceeded its {timeout_ms}ms startup limit"
        ))
    })?
    .map_err(|error| {
        FunctionCallError::RespondToModel(format!("failed to start project AGENT command: {error}"))
    })?;
    let process = started.process;
    let collected =
        tokio::time::timeout_at(deadline, collect_process_output(Arc::clone(&process))).await;
    let (exit_code, stdout, stderr) = match collected {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            let _ = process.terminate().await;
            return Err(FunctionCallError::RespondToModel(format!(
                "project AGENT command exceeded its {timeout_ms}ms limit"
            )));
        }
    };
    let value = json!({
        "exit_code": exit_code,
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": String::from_utf8_lossy(&stderr),
    });
    let output: Box<dyn ToolOutput> =
        Box::new(JsonToolOutput::with_success(value, Some(exit_code == 0)));
    Ok(output)
}

async fn collect_process_output(
    process: Arc<dyn ExecProcess>,
) -> Result<(i32, Vec<u8>, Vec<u8>), FunctionCallError> {
    let mut after_seq = None;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    loop {
        let remaining = MAX_COMMAND_OUTPUT_BYTES
            .saturating_sub(stdout.len() + stderr.len())
            .saturating_add(1);
        let response = process
            .read(
                /*after_seq*/ after_seq,
                /*max_bytes*/ Some(remaining),
                /*wait_ms*/ Some(COMMAND_READ_WAIT_MS),
            )
            .await
            .map_err(|error| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read project AGENT command output: {error}"
                ))
            })?;
        after_seq = Some(response.next_seq);
        for chunk in response.chunks {
            let target = match chunk.stream {
                ExecOutputStream::Stdout | ExecOutputStream::Pty => &mut stdout,
                ExecOutputStream::Stderr => &mut stderr,
            };
            target.extend_from_slice(&chunk.chunk.0);
            if stdout.len() + stderr.len() > MAX_COMMAND_OUTPUT_BYTES {
                let _ = process.terminate().await;
                return Err(FunctionCallError::RespondToModel(format!(
                    "project AGENT command output exceeded {MAX_COMMAND_OUTPUT_BYTES} bytes"
                )));
            }
        }
        if let Some(failure) = response.failure {
            return Err(FunctionCallError::RespondToModel(format!(
                "project AGENT command failed: {failure}"
            )));
        }
        if response.closed {
            return Ok((response.exit_code.unwrap_or(-1), stdout, stderr));
        }
    }
}
