use super::*;
use pretty_assertions::assert_eq;

fn decision(key: &str, content: &str) -> StateEntry {
    StateEntry {
        key: key.to_string(),
        kind: StateEntryKind::UserDecision,
        content: content.to_string(),
        evidence: Vec::new(),
        source_revision: None,
    }
}

fn active(entries: Vec<StateEntry>) -> ActiveContextState {
    ActiveContextState {
        objective: "continue the implementation".to_string(),
        entries,
        constraints: Vec::new(),
        open_questions: Vec::new(),
        next_action: "edit the state projection".to_string(),
    }
}

#[test]
fn state_update_replaces_active_and_promotes_existing_session_memory() {
    let current = ContextStateSnapshot {
        quarter: Vec::new(),
        session: vec![decision("compression.model", "preserve effective state")],
        active: active(Vec::new()),
    };
    let expected = ContextStateSnapshot {
        quarter: vec![decision("compression.model", "preserve effective state")],
        session: vec![decision("cache.prefix", "keep the durable prefix stable")],
        active: active(vec![decision("work.focus", "replace activity summaries")]),
    };

    let actual = apply_context_state_update(
        &current,
        &ContextStateUpdate {
            active: expected.active.clone(),
            session_upserts: expected.session.clone(),
            forget_keys: Vec::new(),
            quarter_promotions: vec!["compression.model".to_string()],
        },
        CheckpointGenerationId::new(SESSIONS_PER_QUARTER),
    )
    .expect("apply state update");

    assert_eq!(actual, expected);
}

#[test]
fn state_update_rejects_immediate_quarter_promotion() {
    let error = apply_context_state_update(
        &ContextStateSnapshot::default(),
        &ContextStateUpdate {
            active: active(Vec::new()),
            session_upserts: vec![decision("compression.model", "preserve effective state")],
            forget_keys: Vec::new(),
            quarter_promotions: vec!["compression.model".to_string()],
        },
        CheckpointGenerationId::new(SESSIONS_PER_QUARTER),
    )
    .expect_err("new session memory cannot be promoted immediately");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
}

#[test]
fn source_facts_require_recallable_evidence() {
    let error = apply_context_state_update(
        &ContextStateSnapshot::default(),
        &ContextStateUpdate {
            active: active(vec![StateEntry {
                key: "runtime.prepare_context_state".to_string(),
                kind: StateEntryKind::SourceFact,
                content: "validates contiguous groups".to_string(),
                evidence: Vec::new(),
                source_revision: Some("64b688afc".to_string()),
            }]),
            session_upserts: Vec::new(),
            forget_keys: Vec::new(),
            quarter_promotions: Vec::new(),
        },
        CheckpointGenerationId::new(1),
    )
    .expect_err("source facts need evidence");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
}

#[test]
fn state_update_rejects_quarter_promotion_before_scheduled_boundary() {
    let current = ContextStateSnapshot {
        quarter: Vec::new(),
        session: vec![decision("compression.model", "preserve effective state")],
        active: active(Vec::new()),
    };

    let error = apply_context_state_update(
        &current,
        &ContextStateUpdate {
            active: active(Vec::new()),
            session_upserts: Vec::new(),
            forget_keys: Vec::new(),
            quarter_promotions: vec!["compression.model".to_string()],
        },
        CheckpointGenerationId::new(SESSIONS_PER_QUARTER - 1),
    )
    .expect_err("quarter promotion before the fifth boundary must fail");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
}
