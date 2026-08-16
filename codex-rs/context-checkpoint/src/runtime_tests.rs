use super::*;
use crate::ActiveContextState;
use crate::ContextStateUpdate;
use crate::StateEntry;
use crate::StateEntryKind;
use crate::ToolGroupDisposition;
use crate::ToolGroupSettlement;
use crate::model_checkpoint_view;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn state_request(
    completed_tool_groups: Vec<ToolGroupId>,
    objective: &str,
    continuity_hint: &str,
) -> UpdateContextStateRequest {
    let tool_group_settlements = completed_tool_groups
        .iter()
        .copied()
        .map(|group_id| ToolGroupSettlement {
            group_id,
            disposition: ToolGroupDisposition::ArchiveOnly,
            state_keys: Vec::new(),
            open_questions: Vec::new(),
        })
        .collect();
    UpdateContextStateRequest {
        active_disposition: ActiveStateDisposition::Replace,
        completed_tool_groups,
        tool_group_settlements,
        state: ContextStateUpdate {
            active: ActiveContextState {
                objective: objective.to_string(),
                entries: Vec::new(),
                constraints: Vec::new(),
                open_questions: Vec::new(),
                continuity_hints: vec![continuity_hint.to_string()],
            },
            session_upserts: Vec::new(),
            removals: Vec::new(),
            quarter_promotions: Vec::new(),
        },
    }
}

#[tokio::test]
async fn continue_reviews_the_turn_without_creating_a_checkpoint() {
    let (directory, runtime) = runtime().await;
    let initial = runtime
        .review_context_state(
            "turn-setup",
            state_request(
                Vec::new(),
                "keep investigating",
                "preserve the known field map",
            ),
            directory.path(),
        )
        .await
        .expect("create initial Active state");
    assert!(matches!(initial, StateReviewOutcome::Committed(_)));
    assert!(runtime.state_review_required("turn-current").await);

    runtime.enforce_state_review("turn-current").await;
    assert_eq!(
        runtime.tool_policy().await,
        CheckpointToolPolicy::CheckpointOnly
    );
    let outcome = runtime
        .review_context_state(
            "turn-current",
            UpdateContextStateRequest {
                active_disposition: ActiveStateDisposition::Continue,
                completed_tool_groups: Vec::new(),
                tool_group_settlements: Vec::new(),
                state: ContextStateUpdate::default(),
            },
            directory.path(),
        )
        .await
        .expect("review unchanged Active state");

    assert_eq!(outcome, StateReviewOutcome::Continued);
    assert!(!runtime.state_review_required("turn-current").await);
    assert_eq!(runtime.tool_policy().await, CheckpointToolPolicy::Normal);
    assert_eq!(
        runtime.rollout_state().await.last_checkpoint,
        Some(TurnRecordId::new(1))
    );
}

#[tokio::test]
async fn replace_and_clear_archive_previous_active_state_for_recall() {
    let (directory, runtime) = runtime().await;
    runtime
        .review_context_state(
            "turn-1",
            state_request(Vec::new(), "map the old field array", "retain offsets"),
            directory.path(),
        )
        .await
        .expect("create initial Active state");

    let replaced = runtime
        .review_context_state(
            "turn-2",
            state_request(Vec::new(), "validate the new field array", "compare builds"),
            directory.path(),
        )
        .await
        .expect("replace Active state");
    let StateReviewOutcome::Committed(replaced_record) = replaced else {
        panic!("state-only replacement should commit immediately");
    };
    let transition = replaced_record
        .active_transition
        .as_ref()
        .expect("replacement should retain previous Active history");
    assert_eq!(transition.disposition, ActiveStateDisposition::Replace);
    assert_eq!(transition.previous_objective, "map the old field array");
    assert!(transition.history_artifact.is_some());
    let view = model_checkpoint_view(&replaced_record).expect("render replacement checkpoint");
    assert!(view.contains("Previous Active"));
    assert!(view.contains("map the old field array"));
    assert!(view.contains("history: artifact:TR000002/A001"));

    let recalled = runtime
        .recall(RecallRequest {
            locator: ArtifactLocator::Reference("artifact:TR000002/A001".to_string()),
            selection: crate::ArtifactRecallSelection::Prefix,
            max_bytes: 4096,
        })
        .await
        .expect("recall replaced Active state");
    let crate::ArtifactRecallView::Prefix { content, .. } = recalled.view else {
        panic!("Active history recall should return a prefix view");
    };
    assert!(content.contains("map the old field array"));
    assert!(content.contains("validate the new field array"));

    let cleared = runtime
        .review_context_state(
            "turn-3",
            UpdateContextStateRequest {
                active_disposition: ActiveStateDisposition::Clear,
                completed_tool_groups: Vec::new(),
                tool_group_settlements: Vec::new(),
                state: ContextStateUpdate::default(),
            },
            directory.path(),
        )
        .await
        .expect("clear Active state");
    let StateReviewOutcome::Committed(cleared_record) = cleared else {
        panic!("state-only clear should commit immediately");
    };
    assert_eq!(cleared_record.state.active, ActiveContextState::default());
    assert_eq!(
        cleared_record
            .active_transition
            .as_ref()
            .expect("clear should retain previous Active history")
            .previous_objective,
        "validate the new field array"
    );
}

fn usage() -> ContextUsageSnapshot {
    ContextUsageSnapshot {
        active_tokens: 80_000,
        context_window: Some(100_000),
        usage_source: crate::UsageSource::Provider,
    }
}

fn invocation(call_id: &str) -> ToolInvocationRecord {
    ToolInvocationRecord {
        call_id: call_id.to_string(),
        tool_name: "shell_command".to_string(),
        media_type: "application/json".to_string(),
        payload: format!(r#"{{"call":"{call_id}"}}"#).into_bytes(),
    }
}

fn successful_result(value: &str, truncation: TruncationProvenance) -> ToolResultRecord {
    ToolResultRecord {
        media_type: "text/plain".to_string(),
        payload: value.as_bytes().to_vec(),
        outcome: ToolCallOutcome::Success,
        truncation,
    }
}

async fn runtime() -> (tempfile::TempDir, CheckpointRuntime) {
    let directory = tempdir().expect("create temporary checkpoint directory");
    let root = AbsolutePathBuf::try_from(directory.path().to_path_buf())
        .expect("temporary directory is absolute");
    let runtime =
        CheckpointRuntime::try_load(CheckpointStore::new(root), CheckpointBudget::default())
            .await
            .expect("load checkpoint runtime");
    (directory, runtime)
}

async fn record_group(
    runtime: &CheckpointRuntime,
    call_id: &str,
    history_start: usize,
    history_end: usize,
    truncation: TruncationProvenance,
) -> ToolCallRecord {
    runtime
        .prepare_request("turn-1", history_start, usage(), 2_000)
        .await
        .expect("prepare sampling request");
    let ArtifactCaptureOutcome::Recorded(record) = runtime
        .record_tool_result(invocation(call_id), successful_result(call_id, truncation))
        .await
    else {
        panic!("tool artifact capture degraded");
    };
    runtime
        .prepare_request("turn-1", history_end, usage(), 2_000)
        .await
        .expect("settle tool group");
    record
}

#[tokio::test]
async fn controlled_settlement_precedes_legacy_compaction() {
    let (_directory, runtime) = runtime().await;
    assert!(!runtime.can_settle_before_legacy_compaction().await);

    record_group(&runtime, "call-1", 1, 3, TruncationProvenance::Complete).await;
    assert!(runtime.can_settle_before_legacy_compaction().await);

    runtime
        .require_legacy_fallback("checkpoint installation failed")
        .await;
    assert!(runtime.is_fallback_required().await);
    assert!(!runtime.can_settle_before_legacy_compaction().await);
}

#[tokio::test]
async fn summary_must_select_the_contiguous_settled_prefix() {
    let (_directory, runtime) = runtime().await;
    let first = record_group(&runtime, "call-1", 0, 2, TruncationProvenance::Complete).await;
    let second = record_group(&runtime, "call-2", 2, 4, TruncationProvenance::Complete).await;

    let error = runtime
        .prepare_context_state(state_request(
            vec![second.group_id],
            "settle the second tool group",
            "continue",
        ))
        .await
        .expect_err("non-contiguous settlement must fail");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
    assert_ne!(first.group_id, second.group_id);
}

#[tokio::test]
async fn archive_only_keeps_truncated_artifacts_recallable() {
    let (_directory, runtime) = runtime().await;
    let call = record_group(
        &runtime,
        "call-1",
        0,
        2,
        TruncationProvenance::ModelFacingFallback,
    )
    .await;

    let pending = runtime
        .prepare_context_state(state_request(
            vec![call.group_id],
            "retain effective knowledge only",
            "the archived output remains selectively recallable",
        ))
        .await
        .expect("truncated process output should archive without forced promotion");

    assert_eq!(
        pending.record.evidence,
        vec![crate::EvidenceRef::Artifact {
            artifact_id: call.output.artifact_id,
        }]
    );
    assert!(
        !model_checkpoint_view(&pending.record)
            .expect("render archive-only checkpoint view")
            .contains("artifact:TR")
    );
}

#[tokio::test]
async fn promoted_groups_attach_output_artifacts_to_state_keys() {
    let (_directory, runtime) = runtime().await;
    let call = record_group(&runtime, "call-1", 0, 2, TruncationProvenance::Complete).await;
    let state_key = "pricing.vip.invariants";
    let symbol = EvidenceRef::Symbol {
        value: "REFERENCE.md::INV-PRICE-01..03".to_string(),
    };
    let mut request = state_request(
        vec![call.group_id],
        "retain verified pricing rules",
        "reuse the verified invariants",
    );
    request.state.active = Default::default();
    request.state.session_upserts = vec![StateEntry {
        key: state_key.to_string(),
        kind: StateEntryKind::SourceFact,
        content: "VIP tax is calculated from the discounted subtotal.".to_string(),
        evidence: vec![symbol.clone()],
        source_revision: None,
    }];
    request.tool_group_settlements = vec![ToolGroupSettlement {
        group_id: call.group_id,
        disposition: ToolGroupDisposition::Promote,
        state_keys: vec![state_key.to_string()],
        open_questions: Vec::new(),
    }];

    let pending = runtime
        .prepare_context_state(request)
        .await
        .expect("promoted output should receive a runtime-owned artifact bridge");
    assert_eq!(
        pending.record.state.session[0].evidence,
        vec![
            symbol,
            EvidenceRef::Artifact {
                artifact_id: call.output.artifact_id,
            },
        ]
    );
    assert!(
        model_checkpoint_view(&pending.record)
            .expect("render model checkpoint view")
            .contains("artifact:TR000001/A001")
    );
}

#[tokio::test]
async fn pending_checkpoint_survives_reload_and_installation() {
    let (directory, runtime) = runtime().await;
    let call = record_group(&runtime, "call-1", 1, 3, TruncationProvenance::Complete).await;
    let pending = runtime
        .prepare_context_state(state_request(
            vec![call.group_id],
            "complete the tool work",
            "answer the user",
        ))
        .await
        .expect("prepare checkpoint");
    assert!(!pending.manifest_sha256.is_empty());
    drop(runtime);

    let root = AbsolutePathBuf::try_from(directory.path().to_path_buf())
        .expect("temporary directory is absolute");
    let restored =
        CheckpointRuntime::try_load(CheckpointStore::new(root), CheckpointBudget::default())
            .await
            .expect("restore checkpoint runtime");
    let restored_pending = restored
        .take_pending_checkpoint()
        .await
        .expect("pending checkpoint restored");
    assert_eq!(restored_pending.record, pending.record);

    restored
        .mark_installed(InstalledCheckpoint {
            record_id: pending.record.record_id,
            replacement_history_sha256: "history-sha".to_string(),
            replacement_item_count: 1,
        })
        .await
        .expect("mark checkpoint installed");
    assert!(restored.take_pending_checkpoint().await.is_none());
    let recalled = restored
        .recall(RecallRequest {
            locator: ArtifactLocator::Reference("artifact:TR000001/A001".to_string()),
            selection: crate::ArtifactRecallSelection::Prefix,
            max_bytes: 1024,
        })
        .await
        .expect("recall artifact after its tool group was settled");
    assert_eq!(
        recalled.view,
        crate::ArtifactRecallView::Prefix {
            line_count: 1,
            content: "call-1".to_string(),
            truncated: false,
        }
    );
}

#[tokio::test]
async fn recall_is_bounded_and_hash_verified() {
    let (_directory, runtime) = runtime().await;
    runtime
        .prepare_request("turn-1", 0, usage(), 0)
        .await
        .expect("prepare sampling request");
    let ArtifactCaptureOutcome::Recorded(call) = runtime
        .record_tool_result(
            invocation("call-1"),
            successful_result("full output", TruncationProvenance::Complete),
        )
        .await
    else {
        panic!("tool artifact capture degraded");
    };

    let recalled = runtime
        .recall(RecallRequest {
            locator: ArtifactLocator::ArtifactId(call.output.artifact_id.clone()),
            selection: crate::ArtifactRecallSelection::Prefix,
            max_bytes: 4,
        })
        .await
        .expect("recall output artifact");

    assert_eq!(
        recalled.view,
        crate::ArtifactRecallView::Prefix {
            line_count: 1,
            content: "full".to_string(),
            truncated: true,
        }
    );
}
