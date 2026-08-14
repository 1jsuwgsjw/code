use super::*;
use codex_context_checkpoint::CheckpointGenerationId;
use codex_context_checkpoint::ToolGroupId;
use codex_context_checkpoint::TurnRecordId;

#[test]
fn renders_a_bounded_typed_checkpoint_fragment() {
    let record = TurnRecord {
        record_id: TurnRecordId::new(2),
        generation_id: CheckpointGenerationId::new(1),
        completed_groups: vec![ToolGroupId::new(3)],
        summary: "inspected the runtime".to_string(),
        evidence: Vec::new(),
        changes: vec!["persisted raw output".to_string()],
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        next_action: "install the checkpoint".to_string(),
        correction_of: None,
    };

    let rendered = CheckpointRecordFragment::new(&record)
        .expect("checkpoint record should serialize")
        .render();

    assert!(rendered.starts_with("<CONTEXT_CHECKPOINT>\n"));
    assert!(rendered.contains("\"recordId\":\"TR000002\""));
    assert!(rendered.ends_with("\n</CONTEXT_CHECKPOINT>"));
}
