use crate::compact::SUMMARY_PREFIX;
use crate::session::Session;
use crate::session::turn_context::TurnContext;
use codex_context_checkpoint::CheckpointError;
use codex_context_checkpoint::InstalledCheckpoint;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::ContextCheckpointRolloutMetadata;
use codex_protocol::protocol::ContextFallbackKind;

pub(crate) async fn fallback_metadata(
    session: &Session,
    fallback_kind: ContextFallbackKind,
    window_number: u64,
) -> ContextCheckpointRolloutMetadata {
    let state = session.services.context_checkpoint.rollout_state().await;
    ContextCheckpointRolloutMetadata {
        checkpoint_id: format!("fallback-{window_number}"),
        generation_id: state.generation_id.to_string(),
        turn_record_id: state
            .last_checkpoint
            .map(|record| record.to_string())
            .unwrap_or_else(|| "none".to_string()),
        manifest_sha256: state
            .manifest_sha256
            .unwrap_or_else(|| "unavailable".to_string()),
        fallback_kind: Some(fallback_kind),
    }
}

pub(crate) async fn install_pending_checkpoint(
    session: &Session,
    turn_context: &TurnContext,
) -> Result<bool, CheckpointError> {
    let Some(pending) = session
        .services
        .context_checkpoint
        .take_pending_checkpoint()
        .await
    else {
        return Ok(false);
    };
    let history = session.clone_history().await;
    let checkpoint_text = format!(
        "{SUMMARY_PREFIX}\n{}",
        serde_json::to_string_pretty(&pending.record)
            .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?
    );
    let checkpoint_item = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: checkpoint_text,
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let replacement_history = codex_context_checkpoint::project_history(
        history.raw_items(),
        &pending,
        checkpoint_item,
    )?;
    let replacement_bytes = serde_json::to_vec(&replacement_history)
        .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?;
    let replacement_history_sha256 =
        codex_context_checkpoint::content_sha256(&replacement_bytes);
    let (window_number, window_ids) = session.advance_auto_compact_window().await;
    let compacted_item = CompactedItem {
        message: pending.record.summary.clone(),
        replacement_history: Some(replacement_history.clone()),
        checkpoint: Some(ContextCheckpointRolloutMetadata {
            checkpoint_id: format!("checkpoint-{}", pending.record.record_id),
            generation_id: pending.record.generation_id.to_string(),
            turn_record_id: pending.record.record_id.to_string(),
            manifest_sha256: pending.manifest_sha256,
            fallback_kind: None,
        }),
        window_number: Some(window_number),
        first_window_id: Some(window_ids.first_window_id.to_string()),
        previous_window_id: window_ids.previous_window_id.map(|id| id.to_string()),
        window_id: Some(window_ids.window_id.to_string()),
    };
    session
        .replace_compacted_history(
            turn_context,
            replacement_history,
            session.reference_context_item().await,
            None,
            compacted_item,
        )
        .await;
    session.recompute_token_usage(turn_context).await;
    if let Err(error) = session
        .services
        .context_checkpoint
        .mark_installed(InstalledCheckpoint {
            record_id: pending.record.record_id,
            replacement_history_sha256,
            replacement_item_count: 1,
        })
        .await
    {
        tracing::warn!(
            error = %error,
            "checkpoint history was installed but Runtime finalization degraded"
        );
    }
    Ok(true)
}
