use crate::ActiveContextState;
use crate::ArtifactCaptureOutcome;
use crate::ArtifactLocator;
use crate::ArtifactRecallSelection;
use crate::ArtifactRecallView;
use crate::CheckpointBudget;
use crate::CheckpointRuntime;
use crate::CheckpointStore;
use crate::ContextStateUpdate;
use crate::ContextUsageSnapshot;
use crate::InstalledCheckpoint;
use crate::RecallRequest;
use crate::ToolCallOutcome;
use crate::ToolGroupDisposition;
use crate::ToolGroupSettlement;
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
            tool_group_settlements: vec![ToolGroupSettlement {
                group_id: call.group_id,
                disposition: ToolGroupDisposition::ArchiveOnly,
                state_keys: Vec::new(),
                open_questions: Vec::new(),
            }],
            state: ContextStateUpdate {
                active: ActiveContextState {
                    objective: "preserve source state in the fork".to_string(),
                    entries: Vec::new(),
                    constraints: Vec::new(),
                    open_questions: Vec::new(),
                    continuity_hints: vec!["Fork state may be relevant later.".to_string()],
                },
                session_upserts: Vec::new(),
                removals: Vec::new(),
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
            locator: ArtifactLocator::ArtifactId(call.output.artifact_id.clone()),
            selection: ArtifactRecallSelection::Prefix,
            max_bytes: 1024,
        })
        .await
        .expect("recall copied artifact");
    assert_eq!(
        recalled.view,
        ArtifactRecallView::Prefix {
            line_count: 1,
            content: "complete output".to_string(),
            truncated: false,
        }
    );
    assert!(target.take_pending_checkpoint().await.is_none());
}

fn usage() -> ContextUsageSnapshot {
    ContextUsageSnapshot {
        active_tokens: 80_000,
        context_window: Some(100_000),
        usage_source: UsageSource::Provider,
    }
}
