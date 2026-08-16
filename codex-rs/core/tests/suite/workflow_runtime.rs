use anyhow::Result;
use codex_protocol::protocol::Op;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::test_codex::test_codex;
use std::sync::Arc;
use wiremock::MockServer;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_injects_structured_workflow_state_without_rewriting_messages() -> Result<()> {
    let server = MockServer::start().await;
    let response_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-before-resume"),
                ev_assistant_message("msg-before-resume", "first result"),
                ev_completed("resp-before-resume"),
            ]),
            sse(vec![
                ev_response_created("resp-after-resume"),
                ev_assistant_message("msg-after-resume", "second result"),
                ev_completed("resp-after-resume"),
            ]),
        ],
    )
    .await;
    let builder = test_codex();
    let initial = builder.build(&server).await?;
    let home = Arc::clone(&initial.home);
    let rollout_path = initial
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");

    initial.submit_turn("before resume").await?;
    initial.codex.submit(Op::Shutdown).await?;

    let resumed = builder.resume(&server, home, rollout_path).await?;
    resumed.submit_turn("after resume").await?;

    let requests = response_mock.requests();
    let resumed_request = requests.last().expect("resumed request").body_json();
    let rendered_input = serde_json::to_string(&resumed_request["input"])?;
    assert!(rendered_input.contains("before resume"));
    assert!(rendered_input.contains("first result"));
    assert!(rendered_input.contains("<WORKFLOW_RUNTIME_STATE>"));
    assert!(rendered_input.contains("completedOperations: 1"));
    assert!(rendered_input.contains("activeOperation: OP000002"));
    assert!(rendered_input.contains("messageReferences:"));
    Ok(())
}
