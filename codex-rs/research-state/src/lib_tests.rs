use super::*;
use pretty_assertions::assert_eq;

fn upsert(id: &str, statement: &str) -> ResearchDelta {
    ResearchDelta {
        operation: ResearchOperation::Upsert,
        id: id.to_string(),
        scope: ResearchScope::Project,
        kind: Some(ResearchEntryKind::Hypothesis),
        subject: Some("theme".to_string()),
        statement: Some(statement.to_string()),
        status: Some(ResearchStatus::Open),
    }
}

#[test]
fn apply_is_transactional_and_revisioned() {
    let mut state = ResearchState::default();
    let first = state
        .apply(&[upsert("theme", "Theme may be a reusable dimension")])
        .expect("first delta should apply");
    assert_eq!(first.revision, 1);
    assert!(first.changed);

    let unchanged = state
        .apply(&[upsert("theme", "Theme may be a reusable dimension")])
        .expect("repeated delta should apply");
    assert_eq!(unchanged.revision, 1);
    assert!(!unchanged.changed);

    let error = state
        .apply(&[
            upsert("layout", "Layout may need a shared scale"),
            ResearchDelta {
                operation: ResearchOperation::SetStatus,
                id: "missing".to_string(),
                scope: ResearchScope::Project,
                kind: None,
                subject: None,
                statement: None,
                status: Some(ResearchStatus::Supported),
            },
        ])
        .expect_err("invalid batch should fail");
    assert_eq!(
        error,
        ResearchStateError::EntryNotFound {
            id: "missing".to_string(),
            scope: ResearchScope::Project,
        }
    );
    assert_eq!(state.revision(), 1);
    assert_eq!(state.entries().len(), 1);
}

#[test]
fn status_updates_and_removals_are_idempotent() {
    let mut state = ResearchState::default();
    state
        .apply(&[upsert("theme", "Theme may be a reusable dimension")])
        .expect("upsert should apply");

    let supported = ResearchDelta {
        operation: ResearchOperation::SetStatus,
        id: "theme".to_string(),
        scope: ResearchScope::Project,
        kind: None,
        subject: None,
        statement: None,
        status: Some(ResearchStatus::Supported),
    };
    state
        .apply(std::slice::from_ref(&supported))
        .expect("status should update");
    let unchanged = state
        .apply(std::slice::from_ref(&supported))
        .expect("repeated status should apply");
    assert!(!unchanged.changed);

    let remove = ResearchDelta {
        operation: ResearchOperation::Remove,
        id: "theme".to_string(),
        scope: ResearchScope::Project,
        kind: None,
        subject: None,
        statement: None,
        status: None,
    };
    state
        .apply(std::slice::from_ref(&remove))
        .expect("remove should apply");
    let repeated_remove = state
        .apply(std::slice::from_ref(&remove))
        .expect("repeated remove should apply");
    assert!(!repeated_remove.changed);
    assert!(state.entries().is_empty());
}

#[test]
fn snapshot_is_a_stable_read_only_projection() {
    let mut state = ResearchState::default();
    state
        .apply(&[upsert("theme", "Theme may be a reusable dimension")])
        .expect("upsert should apply");

    let snapshot = state.snapshot();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].id, "theme");
    assert_eq!(snapshot.entries[0].status, ResearchStatus::Open);
    assert_eq!(snapshot, state.snapshot());
}
