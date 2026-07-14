// CTF|认证 - Research context WorldState tests.

use super::*;
use codex_research_state::ResearchEntry;
use codex_research_state::ResearchEntryKind;
use codex_research_state::ResearchScope;
use codex_research_state::ResearchStatus;

fn projection(statement: &str) -> ResearchContextProjection {
    ResearchContextProjection {
        entries: vec![ResearchEntry {
            id: "ui-layout".to_string(),
            scope: ResearchScope::Project,
            kind: ResearchEntryKind::Decision,
            subject: "UI layout".to_string(),
            statement: statement.to_string(),
            status: ResearchStatus::Supported,
        }],
        omitted_count: 0,
    }
}

#[test]
fn unchanged_projection_does_not_emit_another_fragment() {
    let state = ResearchContextState::new(projection("Keep navigation stable"));
    let snapshot = state.snapshot();

    assert!(
        state
            .render_diff(PreviousSectionState::Known(&snapshot))
            .is_none()
    );
}

#[test]
fn changed_projection_replaces_previous_context() {
    let previous = ResearchContextState::new(projection("Old layout evidence")).snapshot();
    let state = ResearchContextState::new(projection("New layout evidence"));
    let fragment = state
        .render_diff(PreviousSectionState::Known(&previous))
        .expect("replacement fragment");

    assert!(
        fragment
            .render()
            .contains("replaces the previously supplied")
    );
    assert!(fragment.render().contains("New layout evidence"));
}

#[test]
fn omitted_count_does_not_invalidate_model_visible_projection() {
    let mut previous_projection = projection("Keep navigation stable");
    previous_projection.omitted_count = 1;
    let previous = ResearchContextState::new(previous_projection).snapshot();
    let mut current_projection = projection("Keep navigation stable");
    current_projection.omitted_count = 7;
    let current = ResearchContextState::new(current_projection);

    assert!(
        current
            .render_diff(PreviousSectionState::Known(&previous))
            .is_none()
    );
}

#[test]
fn stale_retained_projection_does_not_mask_current_projection() {
    let state = ResearchContextState::new(projection("Current layout evidence"));
    let stale =
        ResearchContextFragment::projected(&projection("Old layout evidence"), false).render();
    let current =
        ResearchContextFragment::projected(&projection("Current layout evidence"), false).render();

    assert!(!state.matches_retained_fragment("user", stale.as_str()));
    assert!(state.matches_retained_fragment("user", current.as_str()));
}

#[test]
fn empty_snapshot_is_a_stable_removal_tombstone() {
    let state = ResearchContextState::default();
    let snapshot = state.snapshot();

    assert!(
        state
            .render_diff(PreviousSectionState::Known(&snapshot))
            .is_none()
    );
}
