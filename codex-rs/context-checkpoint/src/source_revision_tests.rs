use super::*;
use crate::ActiveContextState;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn state_with_source(path: &str) -> ContextStateSnapshot {
    ContextStateSnapshot {
        active: ActiveContextState {
            objective: "preserve current source understanding".to_string(),
            entries: vec![StateEntry {
                key: "source:runtime".to_string(),
                kind: StateEntryKind::SourceFact,
                content: "runtime installs the latest checkpoint".to_string(),
                evidence: vec![EvidenceRef::WorkspacePath {
                    path: path.to_string(),
                }],
                source_revision: None,
            }],
            constraints: Vec::new(),
            open_questions: Vec::new(),
            next_action: "continue implementation".to_string(),
        },
        ..Default::default()
    }
}

#[tokio::test]
async fn source_fact_is_stamped_and_invalidated_after_file_change() {
    let workspace = tempdir().expect("temporary workspace");
    let source_path = workspace.path().join("runtime.rs");
    tokio::fs::write(&source_path, "first revision")
        .await
        .expect("write first revision");
    let mut state = state_with_source("runtime.rs");

    stamp_source_facts(&mut state, workspace.path())
        .await
        .expect("stamp source fact");
    let stamped = state.clone();
    assert!(
        state.active.entries[0]
            .source_revision
            .as_deref()
            .is_some_and(|revision| revision.starts_with("sha256:"))
    );

    invalidate_stale_source_facts(&mut state, workspace.path()).await;
    assert_eq!(state, stamped);

    tokio::fs::write(&source_path, "second revision")
        .await
        .expect("write second revision");
    invalidate_stale_source_facts(&mut state, workspace.path()).await;

    let mut expected = stamped;
    expected.active.entries.clear();
    assert_eq!(state, expected);
}

#[tokio::test]
async fn source_fact_rejects_missing_workspace_evidence() {
    let workspace = tempdir().expect("temporary workspace");
    let mut state = state_with_source("missing.rs");

    let error = stamp_source_facts(&mut state, workspace.path())
        .await
        .expect_err("missing source must be rejected");

    assert!(matches!(error, CheckpointError::UntrustedEvidence(_)));
}
