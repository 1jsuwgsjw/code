use super::*;
use crate::CheckpointGenerationId;
use crate::ToolGroupId;
use crate::TurnRecord;
use crate::TurnRecordId;
use pretty_assertions::assert_eq;

fn pending(start: usize, end: usize) -> PendingCheckpoint {
    PendingCheckpoint {
        record: TurnRecord {
            record_id: TurnRecordId::new(1),
            generation_id: CheckpointGenerationId::new(1),
            completed_groups: vec![ToolGroupId::new(1)],
            tool_group_settlements: Vec::new(),
            state: crate::ContextStateSnapshot::default(),
            state_removals: Vec::new(),
            summary: "summary".to_string(),
            evidence: Vec::new(),
            changes: Vec::new(),
            validation: Vec::new(),
            decisions: Vec::new(),
            open_items: Vec::new(),
            correction_of: None,
        },
        history_start: start,
        history_end: end,
        completed_groups: vec![ToolGroupId::new(1)],
        manifest_sha256: String::new(),
    }
}

#[test]
fn projection_preserves_stable_prefix_and_open_tail() {
    let history = vec!["prefix", "call", "output", "open-call", "open-output"];

    let projected = project_history(&history, &pending(1, 3), "checkpoint")
        .expect("project checkpoint history");

    assert_eq!(
        projected,
        vec!["prefix", "checkpoint", "open-call", "open-output"]
    );
}

#[test]
fn projection_rejects_an_invalid_range() {
    let error = project_history(&["one"], &pending(0, 2), "checkpoint")
        .expect_err("out-of-bounds projection must fail");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
}
