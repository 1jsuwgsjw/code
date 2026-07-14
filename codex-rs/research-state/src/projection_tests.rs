// CTF|认证 - Research context projection tests.

use super::*;
use pretty_assertions::assert_eq;

fn entry(id: &str, subject: &str, statement: &str, kind: ResearchEntryKind) -> ResearchEntry {
    ResearchEntry {
        id: id.to_string(),
        scope: ResearchScope::Project,
        kind,
        subject: subject.to_string(),
        statement: statement.to_string(),
        status: ResearchStatus::Open,
    }
}

#[test]
fn projection_prioritizes_task_relevance_over_generic_priority() {
    let projection = project_research_context(
        &[
            entry(
                "build",
                "release pipeline",
                "Windows builds run in GitHub Actions",
                ResearchEntryKind::Constraint,
            ),
            entry(
                "ui-layout",
                "UI layout",
                "Keep the navigation stable while adjusting the dashboard layout",
                ResearchEntryKind::Hypothesis,
            ),
        ],
        "adjust the UI dashboard layout",
    );

    assert_eq!(projection.entries[0].id, "ui-layout");
    assert_eq!(projection.omitted_count, 0);
}

#[test]
fn projection_excludes_unrelated_project_entries() {
    let projection = project_research_context(
        &[
            entry(
                "build",
                "release pipeline",
                "Windows builds run in GitHub Actions",
                ResearchEntryKind::Constraint,
            ),
            entry(
                "ui-layout",
                "UI layout",
                "Keep navigation stable while adjusting the dashboard layout",
                ResearchEntryKind::Decision,
            ),
        ],
        "study a calculus chapter",
    );

    assert_eq!(projection, ResearchContextProjection::default());
}

#[test]
fn projection_is_bounded_and_project_scoped() {
    let mut entries = (0..20)
        .map(|index| {
            entry(
                format!("entry-{index}").as_str(),
                "bounded context",
                "x".repeat(2_000).as_str(),
                ResearchEntryKind::Fact,
            )
        })
        .collect::<Vec<_>>();
    entries.push(ResearchEntry {
        id: "task-only".to_string(),
        scope: ResearchScope::Task,
        kind: ResearchEntryKind::Constraint,
        subject: "task".to_string(),
        statement: "must not be projected".to_string(),
        status: ResearchStatus::Supported,
    });

    let projection = project_research_context(&entries, "bounded context");
    let projected_bytes = projection
        .entries
        .iter()
        .map(|entry| entry.subject.len() + entry.statement.len() + entry.id.len() + 96)
        .sum::<usize>();

    assert!(projection.entries.len() <= RESEARCH_CONTEXT_MAX_ENTRIES);
    assert!(projected_bytes <= RESEARCH_CONTEXT_MAX_PROJECTED_BYTES);
    assert!(projection.omitted_count > 0);
    assert!(
        projection
            .entries
            .iter()
            .all(|entry| entry.scope == ResearchScope::Project)
    );
}
