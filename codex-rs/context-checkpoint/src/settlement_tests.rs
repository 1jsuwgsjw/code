use super::*;
use crate::ActiveContextState;
use crate::StateEntry;
use crate::StateEntryKind;
use crate::ToolGroupId;
use crate::ToolGroupState;
use pretty_assertions::assert_eq;

fn group() -> ToolGroupRecord {
    ToolGroupRecord {
        group_id: ToolGroupId::new(1),
        turn_id: "turn-1".to_string(),
        history_start: Some(0),
        history_end: Some(1),
        calls: Vec::new(),
        state: ToolGroupState::Settled,
        requires_recall: false,
    }
}

fn state() -> ContextStateSnapshot {
    ContextStateSnapshot {
        session: vec![StateEntry {
            key: "fact:checkpoint-policy".to_string(),
            kind: StateEntryKind::UserDecision,
            content: "Compression retains effective knowledge.".to_string(),
            evidence: Vec::new(),
            source_revision: None,
        }],
        active: ActiveContextState {
            objective: "Preserve knowledge continuity.".to_string(),
            open_questions: vec!["Which TUI view should expose the audit trail?".to_string()],
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn promote_settlement_requires_a_retained_state_key() {
    let group = group();
    let settlement = ToolGroupSettlement {
        group_id: group.group_id,
        disposition: ToolGroupDisposition::Promote,
        state_keys: vec!["fact:checkpoint-policy".to_string()],
        open_questions: Vec::new(),
    };

    assert_eq!(
        validate_tool_group_settlements(&[&group], &[settlement], &state()),
        Ok(())
    );
}

#[test]
fn archive_only_rejects_hidden_durable_knowledge() {
    let group = group();
    let settlement = ToolGroupSettlement {
        group_id: group.group_id,
        disposition: ToolGroupDisposition::ArchiveOnly,
        state_keys: vec!["fact:checkpoint-policy".to_string()],
        open_questions: Vec::new(),
    };

    assert_eq!(
        validate_tool_group_settlements(&[&group], &[settlement], &state()),
        Err(CheckpointError::InvalidRequest(
            "archiveOnly settlement for TG000001 cannot retain state keys or open questions"
                .to_string()
        ))
    );
}

#[test]
fn archive_only_rejects_groups_that_require_recall() {
    let mut group = group();
    group.requires_recall = true;
    let settlement = ToolGroupSettlement {
        group_id: group.group_id,
        disposition: ToolGroupDisposition::ArchiveOnly,
        state_keys: Vec::new(),
        open_questions: Vec::new(),
    };

    assert_eq!(
        validate_tool_group_settlements(&[&group], &[settlement], &state()),
        Err(CheckpointError::InvalidRequest(
            "tool group TG000001 requires recall and cannot be settled as archiveOnly".to_string()
        ))
    );
}
