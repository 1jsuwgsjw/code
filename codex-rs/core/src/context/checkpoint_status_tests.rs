use super::*;
use crate::context::ContextualUserFragment;
use codex_context_checkpoint::CheckpointGenerationId;
use codex_context_checkpoint::ToolGroupId;
use codex_context_checkpoint::UsageSource;
use pretty_assertions::assert_eq;

#[test]
fn renders_bounded_ephemeral_memory_status() {
    let fragment = CheckpointStatusFragment::new(MemoryStatusSnapshot {
        generation_id: CheckpointGenerationId::new(2),
        completed_sessions: 1,
        quarter_consolidation_due: false,
        turn_id: "turn-7".to_string(),
        context_usage_basis_points: 8_512,
        usage_source: UsageSource::Provider,
        tool_result_share_basis_points: 2_050,
        open_groups: 1,
        settled_groups: 2,
        settleable_group_ids: vec![ToolGroupId::new(3), ToolGroupId::new(4)],
        last_checkpoint: None,
        pressure: CheckpointPressure::Required,
        degraded_reason: None,
    });

    assert_eq!(
        fragment.render(),
        "<MEMORY_STATUS>\n\
generation=G000002 turn=turn-7\n\
completed_sessions=1 quarter_consolidation_due=false\n\
context_usage=85.12% usage_source=Provider tool_result_share=20.50%\n\
tool_groups=3 open=1 settled=2 settleable=[TG000003,TG000004]\n\
last_checkpoint=none\n\
             pressure=Required checkpoint_required=true tool=update_context_state recall=recall_checkpoint_artifact\n\
             policy=retain_knowledge+retain_uncertainty+archive_process;no_next_action;settle_each_group\n\
</MEMORY_STATUS>"
    );
}
