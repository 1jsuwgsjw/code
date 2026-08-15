use super::*;
use codex_context_checkpoint::ActiveContextState;
use codex_context_checkpoint::CheckpointGenerationId;
use codex_context_checkpoint::ContextStateSnapshot;
use codex_context_checkpoint::ToolGroupId;
use codex_context_checkpoint::TurnRecordId;

#[test]
fn renders_a_bounded_typed_checkpoint_fragment() {
    let record = TurnRecord {
        record_id: TurnRecordId::new(2),
        generation_id: CheckpointGenerationId::new(1),
        completed_groups: vec![ToolGroupId::new(3)],
        tool_group_settlements: Vec::new(),
        state: ContextStateSnapshot {
            quarter: Vec::new(),
            session: Vec::new(),
            active: ActiveContextState {
                objective: "preserve effective runtime state".to_string(),
                entries: Vec::new(),
                constraints: Vec::new(),
                open_questions: Vec::new(),
                continuity_hints: vec!["The checkpoint state may be relevant later.".to_string()],
            },
        },
        state_removals: Vec::new(),
        summary: String::new(),
        evidence: Vec::new(),
        changes: Vec::new(),
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        correction_of: None,
    };

    let fragment =
        CheckpointRecordFragment::new(&record).expect("checkpoint record should serialize");
    let rendered = fragment.render();
    let item: codex_protocol::models::ResponseItem = fragment.into_response_input_item().into();

    assert!(rendered.starts_with("<CONTEXT_CHECKPOINT>\n"));
    assert!(rendered.contains("\nActive\n"));
    assert!(rendered.contains("    preserve effective runtime state"));
    assert!(!rendered.contains("\"objective\""));
    assert!(!rendered.contains("recordId"));
    assert!(rendered.ends_with("\n</CONTEXT_CHECKPOINT>"));
    assert!(CheckpointRecordFragment::matches_item(&item));
}
