use super::PlanToolOutput;
use super::parse_update_plan_arguments;
use codex_protocol::plan_tool::ResearchEntry;
use codex_protocol::plan_tool::ResearchEntryKind;
use codex_protocol::plan_tool::ResearchOperation;
use codex_protocol::plan_tool::ResearchScope;
use codex_protocol::plan_tool::ResearchStateUpdate;
use codex_protocol::plan_tool::ResearchStatus;
use pretty_assertions::assert_eq;

#[test]
fn parse_update_plan_keeps_research_delta_optional() {
    let args = parse_update_plan_arguments(
        r#"{"plan":[{"step":"Inspect the project","status":"in_progress"}]}"#,
    )
    .expect("legacy update_plan arguments should parse");

    assert_eq!(args.research_delta, None);
}

#[test]
fn parse_update_plan_accepts_research_delta() {
    let args = parse_update_plan_arguments(
        r#"{
            "plan":[{"step":"Inspect the theme system","status":"in_progress"}],
            "research_delta":[{
                "operation":"upsert",
                "id":"theme-as-variable",
                "scope":"project",
                "kind":"hypothesis",
                "subject":"theme",
                "statement":"Theme may be becoming a reusable project dimension",
                "status":"open"
            }]
        }"#,
    )
    .expect("research delta should parse");

    let delta = args
        .research_delta
        .expect("research delta should be present")
        .into_iter()
        .next()
        .expect("one research delta should be present");
    assert_eq!(delta.operation, ResearchOperation::Upsert);
    assert_eq!(delta.scope, ResearchScope::Project);
    assert_eq!(delta.kind, Some(ResearchEntryKind::Hypothesis));
    assert_eq!(delta.status, Some(ResearchStatus::Open));
}

#[test]
fn plan_tool_output_reports_bounded_research_summary() {
    let output = PlanToolOutput {
        research_update: Some(ResearchStateUpdate {
            revision: 2,
            changed: true,
            entries: vec![ResearchEntry {
                id: "theme".to_string(),
                scope: ResearchScope::Project,
                kind: ResearchEntryKind::Hypothesis,
                subject: "theme".to_string(),
                statement: "Theme may be reusable".to_string(),
                status: ResearchStatus::Supported,
            }],
        }),
    };

    assert_eq!(
        output.response_message(),
        "Plan updated; research state revision 2 (changed, 1 entry)"
    );
}

#[test]
fn plan_tool_output_keeps_legacy_message_without_research_delta() {
    let output = PlanToolOutput {
        research_update: None,
    };

    assert_eq!(output.response_message(), "Plan updated");
}
