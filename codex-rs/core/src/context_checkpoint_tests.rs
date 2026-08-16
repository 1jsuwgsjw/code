use super::validate_checkpoint_history_range;
use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;

fn message(role: &str, phase: Option<MessagePhase>, text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn checkpoint_range_allows_only_process_messages() {
    let history = vec![
        message("user", None, "original request"),
        message(
            "assistant",
            Some(MessagePhase::Commentary),
            "temporary tool analysis",
        ),
        message(
            "assistant",
            Some(MessagePhase::FinalAnswer),
            "persistent result",
        ),
    ];

    validate_checkpoint_history_range(&history, 1, 2)
        .expect("assistant commentary may be checkpointed");
}

#[test]
fn checkpoint_range_rejects_authoritative_messages() {
    for item in [
        message("user", None, "original request"),
        message("developer", None, "runtime instructions"),
        message("assistant", None, "legacy final result"),
        message("assistant", Some(MessagePhase::FinalAnswer), "final result"),
    ] {
        let error = validate_checkpoint_history_range(&[item], 0, 1)
            .expect_err("authoritative messages must remain outside checkpoint ranges");
        assert!(error.to_string().contains("persistent"));
    }
}

#[test]
fn checkpoint_range_rejects_invalid_bounds() {
    let history = vec![message("user", None, "original request")];

    let error = validate_checkpoint_history_range(&history, 1, 2)
        .expect_err("an invalid range must not be projected");
    assert!(error.to_string().contains("invalid for history length 1"));
}
