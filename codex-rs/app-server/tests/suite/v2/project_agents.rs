use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadProjectAgentListResponse;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceRunResponse;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceStatusUpdatedNotification;
use codex_app_server_protocol::ThreadProjectAgentReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::ExecutorFileSystem;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const ROOT_ONLY_MESSAGE: &str = "Delegate this root-only request.";
const WORKER_TASK: &str = "Inspect the target";

#[cfg(any(target_os = "macos", windows))]
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(10);

struct ProjectAgentsResponder;

impl Respond for ProjectAgentsResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body = request_body_json(request);
        if body.get("model").and_then(Value::as_str) == Some("worker-model") {
            let task_id = body
                .pointer("/text/format/schema/properties/task_id/enum/0")
                .and_then(Value::as_str)
                .expect("worker request should constrain task_id");
            let result = json!({
                "status": "completed",
                "agent_id": "query",
                "task_id": task_id,
                "result": "Located the requested symbol.",
                "artifacts": ["artifact.txt"],
                "evidence": ["src/lib.rs:42"],
                "memory_candidates": ["The entrypoint is src/lib.rs."],
                "improvement_proposals": ["Add a narrower search helper."],
                "error": null,
            });
            let result = serde_json::to_string(&result).expect("worker result should serialize");
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("resp-worker"),
                responses::ev_assistant_message("msg-worker", &result),
                responses::ev_completed("resp-worker"),
            ]));
        }

        if has_function_call_output(&body, "agent-call") {
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("resp-root-final"),
                responses::ev_assistant_message("msg-root-final", "Delegation complete."),
                responses::ev_completed("resp-root-final"),
            ]));
        }

        responses::sse_response(responses::sse(vec![
            responses::ev_response_created("resp-root"),
            responses::ev_function_call_with_namespace(
                "agent-call",
                "agent",
                "query",
                &serde_json::to_string(&json!({"task": WORKER_TASK}))
                    .expect("delegate arguments should serialize"),
            ),
            responses::ev_completed("resp-root"),
        ]))
    }
}

#[tokio::test]
async fn project_agent_delegate_isolated_worker_and_persists_valid_result() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(ProjectAgentsResponder)
        .mount(&server)
        .await;

    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let (file_system, project_root) = {
        let auto_env = mcp.auto_env()?;
        (
            auto_env.environment().get_filesystem(),
            auto_env.selection().cwd.clone(),
        )
    };
    create_project_agent(file_system.as_ref(), &project_root).await?;

    let thread_req = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let thread_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_req)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(thread_resp)?;
    let project_root_string = project_root.to_string();
    let initial_status =
        notification_params::<ThreadProjectAgentMaintenanceStatusUpdatedNotification>(
            timeout(
                DEFAULT_READ_TIMEOUT,
                mcp.read_stream_until_notification_message(
                    "thread/projectAgentMaintenance/statusUpdated",
                ),
            )
            .await??,
        )?;
    assert_eq!(
        initial_status,
        ThreadProjectAgentMaintenanceStatusUpdatedNotification {
            thread_id: thread.id.clone(),
            project_root: project_root_string.clone(),
            pending_count: 0,
            catalog_revision: 1,
        }
    );

    let turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: ROOT_ONLY_MESSAGE.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_req)),
    )
    .await??;
    let _turn: TurnStartResponse = to_response::<TurnStartResponse>(turn_resp)?;
    let pending_status =
        notification_params::<ThreadProjectAgentMaintenanceStatusUpdatedNotification>(
            timeout(
                DEFAULT_READ_TIMEOUT,
                mcp.read_stream_until_notification_message(
                    "thread/projectAgentMaintenance/statusUpdated",
                ),
            )
            .await??,
        )?;
    assert_eq!(
        pending_status,
        ThreadProjectAgentMaintenanceStatusUpdatedNotification {
            thread_id: thread.id.clone(),
            project_root: project_root_string.clone(),
            pending_count: 2,
            catalog_revision: 1,
        }
    );
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain requests")?;
    let response_requests = requests
        .iter()
        .filter(|request| {
            request.method.as_str() == "POST" && request.url.path().ends_with("/responses")
        })
        .map(|request| (request.url.path().to_string(), request_body_json(request)))
        .collect::<Vec<_>>();
    assert_eq!(
        response_requests
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/root/v1/responses",
            "/worker/v1/responses",
            "/root/v1/responses",
        ]
    );

    let root_request = &response_requests[0].1;
    let worker_request = &response_requests[1].1;
    let root_follow_up = &response_requests[2].1;
    let agent_namespace = root_request["tools"]
        .as_array()
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("name").and_then(Value::as_str) == Some("agent"))
        })
        .context("root request should expose the agent namespace")?;
    assert_eq!(agent_namespace["type"], json!("namespace"));
    assert_eq!(agent_namespace["tools"][0]["name"], json!("query"));

    assert_eq!(worker_request["model"], json!("worker-model"));
    assert_eq!(
        worker_request.pointer("/reasoning/effort"),
        Some(&json!("high"))
    );
    assert_eq!(worker_request["tools"], json!([]));
    assert!(worker_request.to_string().contains(WORKER_TASK));
    assert!(!worker_request.to_string().contains(ROOT_ONLY_MESSAGE));
    let schema = worker_request
        .pointer("/text/format/schema")
        .context("worker request should include its fixed result schema")?;
    assert_eq!(
        schema["required"],
        json!([
            "status",
            "agent_id",
            "task_id",
            "result",
            "artifacts",
            "evidence",
            "memory_candidates",
            "improvement_proposals",
            "error",
        ])
    );
    assert_eq!(schema["properties"]["agent_id"]["enum"], json!(["query"]));
    let task_id = schema["properties"]["task_id"]["enum"][0]
        .as_str()
        .context("worker schema should constrain task_id")?;
    assert!(has_function_call_output(root_follow_up, "agent-call"));

    let expected_result = json!({
        "status": "completed",
        "agent_id": "query",
        "task_id": task_id,
        "result": "Located the requested symbol.",
        "artifacts": ["artifact.txt"],
        "evidence": ["src/lib.rs:42"],
        "memory_candidates": ["The entrypoint is src/lib.rs."],
        "improvement_proposals": ["Add a narrower search helper."],
        "error": null,
    });
    let agent_root = project_root.join("AGENT/agents/query")?;
    assert_eq!(
        read_json(
            file_system.as_ref(),
            &agent_root.join("tasks/current.json")?
        )
        .await?,
        expected_result
    );
    assert_eq!(
        read_json(
            file_system.as_ref(),
            &agent_root.join(&format!("tasks/history/{task_id}.json"))?,
        )
        .await?,
        expected_result
    );
    assert_eq!(
        read_json(
            file_system.as_ref(),
            &agent_root.join(&format!("memory/candidates/{task_id}-0.json"))?,
        )
        .await?,
        json!({
            "schema_version": 1,
            "agent_id": "query",
            "task_id": task_id,
            "body": "The entrypoint is src/lib.rs.",
        })
    );
    assert_eq!(
        read_json(
            file_system.as_ref(),
            &agent_root.join(&format!("proposals/pending/{task_id}-0.json"))?,
        )
        .await?,
        json!({
            "schema_version": 1,
            "agent_id": "query",
            "task_id": task_id,
            "body": "Add a narrower search helper.",
        })
    );

    let session = read_json(file_system.as_ref(), &agent_root.join("session.json")?).await?;
    let current_task = read_json(
        file_system.as_ref(),
        &agent_root.join("tasks/current-task.json")?,
    )
    .await?;
    let api_session = json!({
        "agentId": "query",
        "threadId": session["thread_id"],
        "generation": session["generation"],
        "createdAt": session["created_at"],
        "updatedAt": session["updated_at"],
    });
    let api_task = json!({
        "agentId": "query",
        "taskId": task_id,
        "task": WORKER_TASK,
        "phase": "completed",
        "sessionThreadId": current_task["session_thread_id"],
        "parentThreadId": thread.id,
        "attempt": 1,
        "createdAt": current_task["created_at"],
        "updatedAt": current_task["updated_at"],
    });
    let api_result = json!({
        "status": "completed",
        "agentId": "query",
        "taskId": task_id,
        "result": "Located the requested symbol.",
        "artifacts": ["artifact.txt"],
        "evidence": ["src/lib.rs:42"],
        "memoryCandidates": ["The entrypoint is src/lib.rs."],
        "improvementProposals": ["Add a narrower search helper."],
        "error": null,
    });

    let list_req = mcp
        .send_raw_request(
            "thread/projectAgent/list",
            Some(json!({"threadId": thread.id.clone(), "limit": 1})),
        )
        .await?;
    let list_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(list_req)),
    )
    .await??;
    let list = to_response::<ThreadProjectAgentListResponse>(list_resp)?;
    assert_eq!(
        serde_json::to_value(list)?,
        json!({
            "projectRoot": project_root_string,
            "data": [{
                "id": "query",
                "description": "Performs bounded repository queries.",
                "enabled": true,
                "activeSessionThreadId": session["thread_id"],
                "session": api_session,
                "currentTask": api_task,
            }],
            "nextCursor": "1",
            "total": 2,
        })
    );

    let next_list_req = mcp
        .send_raw_request(
            "thread/projectAgent/list",
            Some(json!({"threadId": thread.id.clone(), "cursor": "1", "limit": 1})),
        )
        .await?;
    let next_list_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(next_list_req)),
    )
    .await??;
    let next_list = to_response::<ThreadProjectAgentListResponse>(next_list_resp)?;
    assert_eq!(
        serde_json::to_value(next_list)?,
        json!({
            "projectRoot": project_root_string,
            "data": [{
                "id": "writer",
                "description": "Writes bounded reports.",
                "enabled": false,
                "activeSessionThreadId": null,
                "session": null,
                "currentTask": null,
            }],
            "nextCursor": null,
            "total": 2,
        })
    );

    let read_req = mcp
        .send_raw_request(
            "thread/projectAgent/read",
            Some(json!({
                "threadId": thread.id.clone(),
                "agentId": "query",
                "taskLimit": 1,
            })),
        )
        .await?;
    let read_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(read_req)),
    )
    .await??;
    let read = to_response::<ThreadProjectAgentReadResponse>(read_resp)?;
    assert_eq!(
        serde_json::to_value(read)?,
        json!({
            "projectRoot": project_root_string,
            "agent": {
                "id": "query",
                "description": "Performs bounded repository queries.",
                "enabled": true,
                "definitionPath": "agents/query/agent.toml",
                "constraintsFile": "constraints.md",
                "model": "worker-model",
                "modelReasoningEffort": "high",
                "modelProvider": "worker_provider",
                "tools": [],
                "memoryMaxItems": 16,
                "memoryMaxTokens": 2000,
            },
            "activeSessionThreadId": session["thread_id"],
            "session": api_session,
            "currentTask": api_task,
            "currentResult": api_result,
            "recentTasks": [api_task],
            "recentResults": [api_result],
        })
    );

    let maintenance_req = mcp
        .send_raw_request(
            "thread/projectAgentMaintenance/run",
            Some(json!({"threadId": thread.id.clone()})),
        )
        .await?;
    let maintenance_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(maintenance_req)),
    )
    .await??;
    let maintenance = to_response::<ThreadProjectAgentMaintenanceRunResponse>(maintenance_resp)?;
    let settled_status = ThreadProjectAgentMaintenanceStatusUpdatedNotification {
        thread_id: thread.id.clone(),
        project_root: project_root_string,
        pending_count: 0,
        catalog_revision: 2,
    };
    assert_eq!(
        maintenance,
        ThreadProjectAgentMaintenanceRunResponse {
            accepted_count: 2,
            rejected_count: 0,
            status: settled_status.clone(),
        }
    );
    let notified_status =
        notification_params::<ThreadProjectAgentMaintenanceStatusUpdatedNotification>(
            timeout(
                DEFAULT_READ_TIMEOUT,
                mcp.read_stream_until_notification_message(
                    "thread/projectAgentMaintenance/statusUpdated",
                ),
            )
            .await??,
        )?;
    assert_eq!(notified_status, settled_status);

    Ok(())
}

#[tokio::test]
async fn project_agent_maintenance_rejects_while_turn_is_running() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(
            responses::sse_response(responses::sse(vec![
                responses::ev_response_created("resp-root-delayed"),
                responses::ev_assistant_message("msg-root-delayed", "Finished."),
                responses::ev_completed("resp-root-delayed"),
            ]))
            .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;

    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let (file_system, project_root) = {
        let auto_env = mcp.auto_env()?;
        (
            auto_env.environment().get_filesystem(),
            auto_env.selection().cwd.clone(),
        )
    };
    create_project_agent(file_system.as_ref(), &project_root).await?;

    let thread_req = mcp
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let thread_resp = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_req)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(thread_resp)?;

    let turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Keep this turn active.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_resp = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_req)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response::<TurnStartResponse>(turn_resp)?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/started"),
    )
    .await??;

    let maintenance_req = mcp
        .send_raw_request(
            "thread/projectAgentMaintenance/run",
            Some(json!({"threadId": thread.id.clone()})),
        )
        .await?;
    let error = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(maintenance_req)),
    )
    .await??;
    assert_eq!(
        error.error.message,
        "project AGENT maintenance is unavailable while a task is running"
    );

    mcp.interrupt_turn_and_wait_for_aborted(thread.id, turn.id, DEFAULT_READ_TIMEOUT)
        .await?;

    Ok(())
}

fn notification_params<T>(notification: JSONRPCNotification) -> Result<T>
where
    T: DeserializeOwned,
{
    let params = notification
        .params
        .context("notification should include params")?;
    serde_json::from_value(params).context("notification params should match the protocol type")
}

fn request_body_json(request: &wiremock::Request) -> Value {
    let body = request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .filter(|encoding| {
            encoding
                .split(',')
                .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
        })
        .map(|_| {
            zstd::stream::decode_all(Cursor::new(&request.body))
                .expect("response request body should be valid zstd")
        })
        .unwrap_or_else(|| request.body.clone());
    serde_json::from_slice(&body).expect("response request body should be JSON")
}

fn has_function_call_output(body: &Value, call_id: &str) -> bool {
    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call_output")
                    && item.get("call_id").and_then(Value::as_str) == Some(call_id)
            })
        })
}

async fn create_project_agent(
    file_system: &dyn ExecutorFileSystem,
    project_root: &PathUri,
) -> Result<()> {
    file_system
        .create_directory(
            &project_root.join(".git")?,
            CreateDirectoryOptions { recursive: true },
            /*sandbox*/ None,
        )
        .await?;
    let agent_root = project_root.join("AGENT/agents/query")?;
    let writer_root = project_root.join("AGENT/agents/writer")?;
    for directory in [&agent_root, &writer_root] {
        file_system
            .create_directory(
                directory,
                CreateDirectoryOptions { recursive: true },
                /*sandbox*/ None,
            )
            .await?;
    }
    file_system
        .write_file(
            &project_root.join("AGENT/registry.toml")?,
            r#"schema_version = 1
revision = 1

[agents.query]
path = "agents/query/agent.toml"
enabled = true

[agents.writer]
path = "agents/writer/agent.toml"
enabled = false
"#
            .as_bytes()
            .to_vec(),
            /*sandbox*/ None,
        )
        .await?;
    file_system
        .write_file(
            &agent_root.join("agent.toml")?,
            r#"schema_version = 1
id = "query"
description = "Performs bounded repository queries."
constraints_file = "constraints.md"
model = "worker-model"
model_reasoning_effort = "high"
model_provider = "worker_provider"
tools = []
memory_max_items = 16
memory_max_tokens = 2000
"#
            .as_bytes()
            .to_vec(),
            /*sandbox*/ None,
        )
        .await?;
    file_system
        .write_file(
            &agent_root.join("constraints.md")?,
            b"Inspect only the target named in the delegated task.".to_vec(),
            /*sandbox*/ None,
        )
        .await?;
    file_system
        .write_file(
            &writer_root.join("agent.toml")?,
            r#"schema_version = 1
id = "writer"
description = "Writes bounded reports."
constraints_file = "constraints.md"
tools = []
memory_max_items = 8
memory_max_tokens = 1000
"#
            .as_bytes()
            .to_vec(),
            /*sandbox*/ None,
        )
        .await?;
    file_system
        .write_file(
            &writer_root.join("constraints.md")?,
            b"Write only the report requested by the delegated task.".to_vec(),
            /*sandbox*/ None,
        )
        .await?;
    Ok(())
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "root-model"
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_provider = "root_provider"

[model_providers.root_provider]
name = "Root mock provider"
base_url = "{server_uri}/root/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0

[model_providers.worker_provider]
name = "Worker mock provider"
base_url = "{server_uri}/worker/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}

async fn read_json(file_system: &dyn ExecutorFileSystem, path: &PathUri) -> Result<Value> {
    let contents = file_system.read_file(path, /*sandbox*/ None).await?;
    Ok(serde_json::from_slice(&contents)?)
}
