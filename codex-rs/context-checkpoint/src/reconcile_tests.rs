use crate::ActiveContextState;
use crate::ArtifactCaptureOutcome;
use crate::CheckpointBudget;
use crate::CheckpointRuntime;
use crate::CheckpointStore;
use crate::ContextStateUpdate;
use crate::ContextUsageSnapshot;
use crate::InstalledCheckpoint;
use crate::RecallRequest;
use crate::ToolCallOutcome;
use crate::ToolInvocationRecord;
use crate::ToolResultRecord;
use crate::TruncationProvenance;
use crate::UpdateContextStateRequest;
use crate::UsageSource;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn fork_reconciliation_copies_checkpoint_dependencies() {
    let directory = tempdir().expect("create checkpoint root");
    let root = AbsolutePathBuf::try_from(directory.path().join("context-checkpoints"))
        .expect("temporary checkpoint root is absolute");
    let source = CheckpointRuntime::try_load(
        CheckpointStore::new(root.join("source")),
        CheckpointBudget::default(),
    )
    .await
    .expect("create source runtime");
    source
        .prepare_request("turn-1", 0, usage(), 0)
        .await
        .expect("start source request");
    let ArtifactCaptureOutcome::Recorded(call) = source
        .record_tool_result(
            ToolInvocationRecord {
                call_id: "call-1".to_string(),
                tool_name: "shell".to_string(),
                media_type: "application/json".to_string(),
                payload: br#"{"command":"status"}"#.to_vec(),
            },
            ToolResultRecord {
                media_type: "text/plain".to_string(),
                payload: b"complete output".to_vec(),
                outcome: ToolCallOutcome::Success,
                truncation: TruncationProvenance::Complete,
            },
        )
        .await
    else {
        panic!("source artifact capture degraded");
    };
    source
        .prepare_request("turn-1", 3, usage(), 0)
        .await
        .expect("settle source tool group");
    let pending = source
        .prepare_context_state(UpdateContextStateRequest {
            completed_tool_groups: vec![call.group_id],
            state: ContextStateUpdate {
                active: ActiveContextState {
                    objective: "preserve source state in the fork".to_string(),
                    entries: Vec::new(),
                    constraints: Vec::new(),
                    open_questions: Vec::new(),
                    next_action: "continue in fork".to_string(),
                },
                session_upserts: Vec::new(),
                forget_keys: Vec::new(),
                quarter_promotions: Vec::new(),
            },
        })
        .await
        .expect("prepare source checkpoint");

    let target = CheckpointRuntime::try_load(
        CheckpointStore::new(root.join("target")),
        CheckpointBudget::default(),
    )
    .await
    .expect("create target runtime");
    target
        .reconcile_reconstructed_checkpoint(
            "source",
            &pending.manifest_sha256,
            pending.record.generation_id,
            InstalledCheckpoint {
                record_id: pending.record.record_id,
                replacement_history_sha256: "replacement-history".to_string(),
                replacement_item_count: 1,
            },
        )
        .await
        .expect("reconcile fork checkpoint");

    let recalled = target
        .recall(RecallRequest {
            artifact_id: call.output.artifact_id,
            max_bytes: 1024,
        })
        .await
        .expect("recall copied artifact");
    assert_eq!(recalled.data, b"complete output".to_vec());
    assert!(target.take_pending_checkpoint().await.is_none());
}

fn usage() -> ContextUsageSnapshot {
    ContextUsageSnapshot {
        active_tokens: 80_000,
        context_window: Some(100_000),
        usage_source: UsageSource::Provider,
    }
}
