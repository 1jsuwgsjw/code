use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::ProjectAgentTaskPhase;
use codex_app_server_protocol::ProjectAgentTaskStatus;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadProjectAgentFollowUpResponse;
use codex_app_server_protocol::ThreadProjectAgentReadResponse;
use codex_app_server_protocol::ThreadProjectAgentRebuildResponse;
use codex_app_server_protocol::ThreadProjectAgentRetryResponse;
use codex_app_server_protocol::ThreadProjectAgentTerminateResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput as V2UserInput;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::sleep;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

use super::project_agents::DEFAULT_READ_TIMEOUT;
use super::project_agents::create_config_toml;
use super::project_agents::create_project_agent;
use super::project_agents::has_function_call_output;
use super::project_agents::request_body_json;

const CONTROL_TASK: &str = "Inspect the control target";
const FOLLOW_UP_MESSAGE: &str = "Also verify the follow-up marker";
const AGENT_CALL_ID: &str = "agent-control-call";

struct ProjectAgentControlResponder {
    worker_requests: Arc<AtomicUsize>,
    first_worker_delay: Duration,
}

impl Respond for ProjectAgentControlResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body = request_body_json(request);
        if body.get("model").and_then(Value::as_str) == Some("worker-model") {
            let request_index = self.worker_requests.fetch_add(1, Ordering::SeqCst);
            let task_id = body
                .pointer("/text/format/schema/properties/task_id/enum/0")
                .and_then(Value::as_str)
                .expect("worker request should constrain task_id");
            let result = serde_json::to_string(&json!({
                "status": "completed",
                "agent_id": "query",
                "task_id": task_id,
                "result": format!("Completed worker request {}.", request_index + 1),
                "artifacts": [],
                "evidence": [],
                "memory_candidates": [],
                "improvement_proposals": [],
                "error": null,
            }))
            .expect("worker result should serialize");
            let response = responses::sse_response(responses::sse(vec![
                responses::ev_response_created(&format!("resp-worker-{request_index}")),
                responses::ev_assistant_message(&format!("msg-worker-{request_index}"), &result),
                responses::ev_completed(&format!("resp-worker-{request_index}")),
            ]));
            return if request_index == 0 {
                response.set_delay(self.first_worker_delay)
            } else {
                response
            };
        }

        if has_function_call_output(&body, AGENT_CALL_ID) {
            return responses::sse_response(responses::sse(vec![
                responses::ev_response_created("resp-root-final"),
                responses::ev_assistant_message("msg-root-final", "Control task finished."),
                responses::ev_completed("resp-root-final"),
            ]));
        }

        responses::sse_response(responses::sse(vec![
            responses::ev_response_created("resp-root-control"),
            responses::ev_function_call_with_namespace(
                AGENT_CALL_ID,
                "agent",
                "query",
                &serde_json::to_string(&json!({"task": CONTROL_TASK}))
                    .expect("delegate arguments should serialize"),
            ),
            responses::ev_completed("resp-root-control"),
        ]))
    }
}

#[tokio::test]
async fn project_agent_follow_up_reaches_the_active_reused_worker() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let worker_requests = Arc::new(AtomicUsize::new(0));
    let (server, mut mcp, thread_id, _codex_home) = start_control_app(
        Arc::clone(&worker_requests),
        Duration::from_secs(/*secs*/ 5),
    )
    .await?;
    start_delegated_turn(&mut mcp, &thread_id).await?;
    wait_for_worker_requests(&worker_requests, /*expected*/ 1).await?;

    let follow_up: ThreadProjectAgentFollowUpResponse = send_request(
        &mut mcp,
        "thread/projectAgent/followUp",
        json!({
            "threadId": thread_id,
            "agentId": "query",
            "message": FOLLOW_UP_MESSAGE,
        }),
    )
    .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    wait_for_worker_requests(&worker_requests, /*expected*/ 2).await?;

    let requests = server
        .received_requests()
        .await
        .context("mock server should retain requests")?;
    let worker_bodies = requests
        .iter()
        .map(request_body_json)
        .filter(|body| body.get("model").and_then(Value::as_str) == Some("worker-model"))
        .collect::<Vec<_>>();
    assert_eq!(worker_bodies.len(), 2);
    assert!(worker_bodies[1].to_string().contains(FOLLOW_UP_MESSAGE));

    let detail = read_agent(&mut mcp, &thread_id).await?;
    let current_task = detail.current_task.context("current task should exist")?;
    assert_eq!(current_task.task_id, follow_up.task_id);
    assert_eq!(current_task.phase, ProjectAgentTaskPhase::Completed);
    assert_eq!(
        current_task.session_thread_id,
        Some(follow_up.session_thread_id.clone())
    );
    assert_eq!(
        detail.active_session_thread_id,
        Some(follow_up.session_thread_id)
    );
    Ok(())
}

#[tokio::test]
async fn project_agent_terminate_retry_and_explicit_rebuild_are_durable() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let worker_requests = Arc::new(AtomicUsize::new(0));
    let (_server, mut mcp, thread_id, _codex_home) = start_control_app(
        Arc::clone(&worker_requests),
        Duration::from_secs(/*secs*/ 30),
    )
    .await?;
    start_delegated_turn(&mut mcp, &thread_id).await?;
    wait_for_worker_requests(&worker_requests, /*expected*/ 1).await?;

    let running = read_agent(&mut mcp, &thread_id).await?;
    let running_task = running.current_task.context("running task should exist")?;
    let original_session = running.session.context("worker session should exist")?;
    assert_eq!(running_task.phase, ProjectAgentTaskPhase::Running);

    let terminated: ThreadProjectAgentTerminateResponse = send_request(
        &mut mcp,
        "thread/projectAgent/terminate",
        json!({"threadId": thread_id, "agentId": "query"}),
    )
    .await?;
    assert_eq!(terminated.task_id, running_task.task_id);
    assert_eq!(terminated.session_thread_id, original_session.thread_id);
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let interrupted =
        wait_for_task_phase(&mut mcp, &thread_id, ProjectAgentTaskPhase::Interrupted).await?;
    let interrupted_result = interrupted
        .current_result
        .context("interrupted result should be persisted")?;
    assert_eq!(interrupted_result.status, ProjectAgentTaskStatus::Failed);
    assert!(
        interrupted_result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("interrupted"))
    );

    let latest_retry: ThreadProjectAgentRetryResponse = send_request(
        &mut mcp,
        "thread/projectAgent/retry",
        json!({"threadId": thread_id, "agentId": "query"}),
    )
    .await?;
    assert_eq!(latest_retry.task.attempt, 2);
    assert_eq!(latest_retry.task.task, CONTROL_TASK);
    let latest_completed =
        wait_for_task_phase(&mut mcp, &thread_id, ProjectAgentTaskPhase::Completed).await?;
    assert_eq!(
        latest_completed
            .current_task
            .as_ref()
            .and_then(|task| task.session_thread_id.as_deref()),
        Some(original_session.thread_id.as_str())
    );

    let selected_retry: ThreadProjectAgentRetryResponse = send_request(
        &mut mcp,
        "thread/projectAgent/retry",
        json!({
            "threadId": thread_id,
            "agentId": "query",
            "taskId": running_task.task_id,
        }),
    )
    .await?;
    assert_eq!(selected_retry.task.attempt, 2);
    let selected_completed = wait_for_task_id(
        &mut mcp,
        &thread_id,
        &selected_retry.task.task_id,
        ProjectAgentTaskPhase::Completed,
    )
    .await?;
    assert_eq!(
        selected_completed.active_session_thread_id.as_deref(),
        Some(original_session.thread_id.as_str())
    );

    let rebuilt: ThreadProjectAgentRebuildResponse = send_request(
        &mut mcp,
        "thread/projectAgent/rebuild",
        json!({"threadId": thread_id, "agentId": "query"}),
    )
    .await?;
    assert_eq!(
        rebuilt.previous_session_thread_id.as_deref(),
        Some(original_session.thread_id.as_str())
    );
    assert_ne!(rebuilt.session.thread_id, original_session.thread_id);
    assert_eq!(
        rebuilt.session.generation,
        original_session.generation.saturating_add(/*rhs*/ 1)
    );
    let rebuilt_detail = read_agent(&mut mcp, &thread_id).await?;
    assert_eq!(
        rebuilt_detail.active_session_thread_id.as_deref(),
        Some(rebuilt.session.thread_id.as_str())
    );
    assert_eq!(rebuilt_detail.session, Some(rebuilt.session));
    Ok(())
}

async fn start_control_app(
    worker_requests: Arc<AtomicUsize>,
    first_worker_delay: Duration,
) -> Result<(wiremock::MockServer, TestAppServer, String, TempDir)> {
    let server = responses::start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(ProjectAgentControlResponder {
            worker_requests,
            first_worker_delay,
        })
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
    Ok((server, mcp, thread.id, codex_home))
}

async fn start_delegated_turn(mcp: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let turn_req = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "Run the project AGENT control task.".to_string(),
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
    let _turn: TurnStartResponse = to_response::<TurnStartResponse>(turn_resp)?;
    Ok(())
}

async fn read_agent(
    mcp: &mut TestAppServer,
    thread_id: &str,
) -> Result<ThreadProjectAgentReadResponse> {
    send_request(
        mcp,
        "thread/projectAgent/read",
        json!({"threadId": thread_id, "agentId": "query", "taskLimit": 10}),
    )
    .await
}

async fn send_request<T>(mcp: &mut TestAppServer, method_name: &str, params: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    let request_id = mcp.send_raw_request(method_name, Some(params)).await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    to_response(response)
}

async fn wait_for_worker_requests(counter: &AtomicUsize, expected: usize) -> Result<()> {
    timeout(DEFAULT_READ_TIMEOUT, async {
        while counter.load(Ordering::SeqCst) < expected {
            sleep(Duration::from_millis(/*millis*/ 25)).await;
        }
    })
    .await
    .with_context(|| format!("timed out waiting for {expected} worker requests"))?;
    Ok(())
}

async fn wait_for_task_phase(
    mcp: &mut TestAppServer,
    thread_id: &str,
    phase: ProjectAgentTaskPhase,
) -> Result<ThreadProjectAgentReadResponse> {
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let detail = read_agent(mcp, thread_id).await?;
            if detail.current_task.as_ref().map(|task| task.phase) == Some(phase) {
                return Ok(detail);
            }
            sleep(Duration::from_millis(/*millis*/ 25)).await;
        }
    })
    .await
    .with_context(|| format!("timed out waiting for project AGENT phase {phase:?}"))?
}

async fn wait_for_task_id(
    mcp: &mut TestAppServer,
    thread_id: &str,
    task_id: &str,
    phase: ProjectAgentTaskPhase,
) -> Result<ThreadProjectAgentReadResponse> {
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let detail = read_agent(mcp, thread_id).await?;
            if detail
                .current_task
                .as_ref()
                .is_some_and(|task| task.task_id == task_id && task.phase == phase)
            {
                return Ok(detail);
            }
            sleep(Duration::from_millis(/*millis*/ 25)).await;
        }
    })
    .await
    .with_context(|| format!("timed out waiting for project AGENT task {task_id}"))?
}
