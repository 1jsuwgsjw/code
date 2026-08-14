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
        state: ContextStateSnapshot {
            quarter: Vec::new(),
            session: Vec::new(),
            active: ActiveContextState {
                objective: "preserve effective runtime state".to_string(),
                entries: Vec::new(),
                constraints: Vec::new(),
                open_questions: Vec::new(),
                next_action: "install the checkpoint".to_string(),
            },
        },
        summary: String::new(),
        evidence: Vec::new(),
        changes: Vec::new(),
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        next_action: String::new(),
        correction_of: None,
    };

    let fragment =
        CheckpointRecordFragment::new(&record).expect("checkpoint record should serialize");
    let rendered = fragment.render();
    let item: codex_protocol::models::ResponseItem = fragment.into_response_input_item().into();

    assert!(rendered.starts_with("<CONTEXT_CHECKPOINT>\n"));
    assert!(rendered.contains("\"objective\":\"preserve effective runtime state\""));
    assert!(!rendered.contains("recordId"));
    assert!(rendered.ends_with("\n</CONTEXT_CHECKPOINT>"));
    assert!(CheckpointRecordFragment::matches_item(&item));
}
