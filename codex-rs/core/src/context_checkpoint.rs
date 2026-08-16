use crate::context::CheckpointRecordFragment;
use crate::session::Session;
use crate::session::turn_context::TurnContext;
use codex_context_checkpoint::CheckpointError;
use codex_context_checkpoint::InstalledCheckpoint;
use codex_context_fragments::ContextualUserFragment;
use codex_protocol::items::ContextCheckpointDetails;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::ContextCheckpointRolloutMetadata;
use codex_protocol::protocol::ContextFallbackKind;

#[derive(Debug)]
pub(crate) struct ReconstructedCheckpoint {
    pub(crate) metadata: ContextCheckpointRolloutMetadata,
    pub(crate) replacement_history_sha256: String,
    pub(crate) replacement_item_count: usize,
}

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
        source_thread_id: Some(session.thread_id().to_string()),
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
    validate_checkpoint_history_range(
        history.raw_items(),
        pending.history_start,
        pending.history_end,
    )?;
    let checkpoint_item =
        CheckpointRecordFragment::new(&pending.record)?.into_response_input_item();
    let replacement_history = codex_context_checkpoint::project_history(
        history.raw_items(),
        &pending,
        checkpoint_item.into(),
    )?;
    let replacement_bytes = serde_json::to_vec(&replacement_history)
        .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?;
    let replacement_history_sha256 = codex_context_checkpoint::content_sha256(&replacement_bytes);
    let (window_number, window_ids) = session.advance_auto_compact_window().await;
    let checkpoint_ref = codex_context_checkpoint::checkpoint_reference(
        pending.record.generation_id,
        pending.record.record_id,
    );
    let first_window_id = window_ids.first_window_id.to_string();
    let previous_window_id = window_ids.previous_window_id.map(|id| id.to_string());
    let window_id = window_ids.window_id.to_string();
    let checkpoint_details = ContextCheckpointDetails {
        checkpoint_ref: checkpoint_ref.clone(),
        generation_id: pending.record.generation_id.to_string(),
        turn_record_id: pending.record.record_id.to_string(),
        labels: codex_context_checkpoint::checkpoint_labels(&pending.record),
        state_entry_count: codex_context_checkpoint::checkpoint_state_entry_count(&pending.record),
        window_number,
        first_window_id: first_window_id.clone(),
        previous_window_id: previous_window_id.clone(),
        window_id: window_id.clone(),
    };
    let compacted_item = CompactedItem {
        message: if pending.record.state.is_empty() {
            pending.record.summary.clone()
        } else {
            let objective = &pending.record.state.active.objective;
            let continuity_hints = &pending.record.state.active.continuity_hints;
            if continuity_hints.is_empty() {
                format!("Context state: {objective}")
            } else {
                format!(
                    "Context state: {objective}\nContinuity hints: {}",
                    continuity_hints.join("; ")
                )
            }
        },
        replacement_history: Some(replacement_history.clone()),
        checkpoint: Some(ContextCheckpointRolloutMetadata {
            checkpoint_id: checkpoint_ref,
            generation_id: pending.record.generation_id.to_string(),
            turn_record_id: pending.record.record_id.to_string(),
            manifest_sha256: pending.manifest_sha256,
            source_thread_id: Some(session.thread_id().to_string()),
            fallback_kind: None,
        }),
        window_number: Some(window_number),
        first_window_id: Some(first_window_id),
        previous_window_id,
        window_id: Some(window_id),
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
    let checkpoint_item =
        TurnItem::ContextCompaction(ContextCompactionItem::with_checkpoint(checkpoint_details));
    session
        .emit_turn_item_started(turn_context, &checkpoint_item)
        .await;
    session
        .emit_turn_item_completed(turn_context, checkpoint_item)
        .await;
    Ok(true)
}

fn validate_checkpoint_history_range(
    history: &[ResponseItem],
    history_start: usize,
    history_end: usize,
) -> Result<(), CheckpointError> {
    let selected = history.get(history_start..history_end).ok_or_else(|| {
        CheckpointError::InvalidRequest(format!(
            "checkpoint range {history_start}..{history_end} is invalid for history length {}",
            history.len()
        ))
    })?;

    for (relative_index, item) in selected.iter().enumerate() {
        let ResponseItem::Message { role, phase, .. } = item else {
            continue;
        };
        if role == "assistant" && matches!(phase, Some(MessagePhase::Commentary)) {
            continue;
        }

        let history_index = history_start + relative_index;
        return Err(CheckpointError::InvalidRequest(format!(
            "checkpoint range contains persistent {role} message at history index {history_index}"
        )));
    }

    Ok(())
}

pub(crate) async fn reconcile_reconstructed_checkpoint(
    session: &Session,
    checkpoint: Option<&ReconstructedCheckpoint>,
    had_checkpoint_metadata: bool,
) -> Result<(), CheckpointError> {
    let Some(checkpoint) = checkpoint else {
        if had_checkpoint_metadata {
            session
                .services
                .context_checkpoint
                .clear_reconstructed_checkpoint()
                .await?;
        }
        return Ok(());
    };

    let generation_id = checkpoint
        .metadata
        .generation_id
        .parse::<codex_context_checkpoint::CheckpointGenerationId>()
        .map_err(CheckpointError::InvalidRequest)?;
    let record_id = checkpoint
        .metadata
        .turn_record_id
        .parse::<codex_context_checkpoint::TurnRecordId>()
        .map_err(CheckpointError::InvalidRequest)?;
    let source_thread_id = checkpoint
        .metadata
        .source_thread_id
        .clone()
        .unwrap_or_else(|| session.thread_id().to_string());
    session
        .services
        .context_checkpoint
        .reconcile_reconstructed_checkpoint(
            &source_thread_id,
            &checkpoint.metadata.manifest_sha256,
            generation_id,
            InstalledCheckpoint {
                record_id,
                replacement_history_sha256: checkpoint.replacement_history_sha256.clone(),
                replacement_item_count: checkpoint.replacement_item_count,
            },
        )
        .await
}

#[cfg(test)]
#[path = "context_checkpoint_tests.rs"]
mod tests;
