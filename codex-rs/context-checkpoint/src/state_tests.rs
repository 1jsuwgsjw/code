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
        continuity_hints: vec!["The projection policy may be reused later.".to_string()],
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
            removals: Vec::new(),
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
            removals: Vec::new(),
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
            removals: Vec::new(),
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
            removals: Vec::new(),
            quarter_promotions: vec!["compression.model".to_string()],
        },
        CheckpointGenerationId::new(SESSIONS_PER_QUARTER - 1),
    )
    .expect_err("quarter promotion before the fifth boundary must fail");

    assert!(matches!(error, CheckpointError::InvalidRequest(_)));
}

#[test]
fn state_update_can_clear_active_after_semantic_closure() {
    let current = ContextStateSnapshot {
        session: vec![decision("session.fact", "retain this project fact")],
        active: active(Vec::new()),
        ..Default::default()
    };
    let expected = ContextStateSnapshot {
        session: current.session.clone(),
        ..Default::default()
    };

    assert_eq!(
        apply_context_state_update(
            &current,
            &ContextStateUpdate {
                active: ActiveContextState::default(),
                session_upserts: Vec::new(),
                removals: Vec::new(),
                quarter_promotions: Vec::new(),
            },
            CheckpointGenerationId::new(2),
        ),
        Ok(expected)
    );
}

#[test]
fn state_removal_requires_a_retained_superseding_entry() {
    let current = ContextStateSnapshot {
        session: vec![decision("compression.policy.v1", "retain task summaries")],
        ..Default::default()
    };
    let replacement = decision(
        "compression.policy.v2",
        "retain effective knowledge and archive process noise",
    );

    let actual = apply_context_state_update(
        &current,
        &ContextStateUpdate {
            active: active(Vec::new()),
            session_upserts: vec![replacement.clone()],
            removals: vec![StateRemoval {
                key: "compression.policy.v1".to_string(),
                reason: StateRemovalReason::Superseded,
                superseded_by: Some("compression.policy.v2".to_string()),
            }],
            quarter_promotions: Vec::new(),
        },
        CheckpointGenerationId::new(2),
    )
    .expect("superseded state should retain its replacement");

    assert_eq!(actual.session, vec![replacement]);
}
