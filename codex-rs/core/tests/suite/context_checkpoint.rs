use anyhow::Result;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::test_codex::TestCodexHarness;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

fn update_context_state_arguments(completed_groups: &[&str], objective: &str) -> String {
    let tool_group_settlements = completed_groups
        .iter()
        .map(|group_id| {
            json!({
                "groupId": group_id,
                "disposition": "archiveOnly",
                "stateKeys": [],
                "openQuestions": []
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "completedToolGroups": completed_groups,
        "toolGroupSettlements": tool_group_settlements,
        "state": {
            "active": {
                "objective": objective,
                "entries": [],
                "constraints": [],
                "openQuestions": [],
                "continuityHints": ["The projected state may be relevant later."]
            },
            "sessionUpserts": [],
            "removals": [],
            "quarterPromotions": []
        }
    }))
    .expect("serialize update_context_state arguments")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_request_contains_only_the_latest_context_state_projection() -> Result<()> {
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.4")).await?;
    let first_update = update_context_state_arguments(&["TG000001"], "first objective");
    let second_update = update_context_state_arguments(&["TG000002"], "second objective");
    let shell_arguments = serde_json::to_string(&json!({ "command": "echo checkpoint" }))?;
    let responses = vec![
        sse(vec![
            ev_response_created("resp-1"),
            ev_function_call("call-shell", "shell_command", &shell_arguments),
            ev_completed("resp-1"),
        ]),
        sse(vec![
            ev_response_created("resp-2"),
            ev_function_call("call-state-1", "update_context_state", &first_update),
            ev_completed("resp-2"),
        ]),
        sse(vec![
            ev_response_created("resp-3"),
            ev_function_call("call-state-2", "update_context_state", &second_update),
            ev_completed("resp-3"),
        ]),
        sse(vec![
            ev_response_created("resp-4"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-4"),
        ]),
    ];
    let mock = mount_sse_sequence(harness.server(), responses).await;

    harness
        .submit("checkpoint the current project state")
        .await?;

    let requests = mock.requests();
    assert_eq!(requests.len(), 4);
    let first_update_output: serde_json::Value = serde_json::from_str(
        &mock
            .function_call_output_text("call-state-1")
            .expect("first checkpoint tool output"),
    )?;
    assert_eq!(first_update_output["checkpointRef"], "ctx:G000001/TR000001");
    assert!(first_update_output.get("manifestSha256").is_none());
    let final_request = requests
        .last()
        .expect("final model request")
        .body_json()
        .to_string();
    assert_eq!(final_request.matches("<CONTEXT_CHECKPOINT>").count(), 1);
    assert!(final_request.contains("\\nActive\\n"));
    assert!(final_request.contains("second objective"));
    assert!(final_request.contains("ctx:G000002/TR000002"));
    assert!(!final_request.contains("\"objective\":"));
    assert!(!final_request.contains("manifestSha256"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checkpoint_artifact_recall_defaults_to_a_structured_outline() -> Result<()> {
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.4")).await?;
    let update = update_context_state_arguments(&["TG000001"], "inspect captured evidence");
    let shell_arguments = serde_json::to_string(&json!({ "command": "echo checkpoint" }))?;
    let recall_arguments = serde_json::to_string(&json!({
        "reference": "artifact:TR000001/A001"
    }))?;
    let responses = vec![
        sse(vec![
            ev_response_created("resp-outline-1"),
            ev_function_call("call-outline-shell", "shell_command", &shell_arguments),
            ev_completed("resp-outline-1"),
        ]),
        sse(vec![
            ev_response_created("resp-outline-2"),
            ev_function_call("call-outline-state", "update_context_state", &update),
            ev_completed("resp-outline-2"),
        ]),
        sse(vec![
            ev_response_created("resp-outline-3"),
            ev_function_call(
                "call-outline-recall",
                "recall_checkpoint_artifact",
                &recall_arguments,
            ),
            ev_completed("resp-outline-3"),
        ]),
        sse(vec![
            ev_response_created("resp-outline-4"),
            ev_assistant_message("msg-outline", "done"),
            ev_completed("resp-outline-4"),
        ]),
    ];
    let mock = mount_sse_sequence(harness.server(), responses).await;

    harness.submit("inspect the saved artifact").await?;

    let output: serde_json::Value = serde_json::from_str(
        &mock
            .function_call_output_text("call-outline-recall")
            .expect("recall tool output"),
    )?;
    assert_eq!(output["view"]["mode"], "outline");
    assert!(
        !output["view"]["sections"]
            .as_array()
            .expect("outline sections")
            .is_empty()
    );
    assert!(output["view"].get("content").is_none());
    Ok(())
}
