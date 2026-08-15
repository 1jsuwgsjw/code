use super::ContextualUserFragment;
use codex_context_checkpoint::CheckpointPressure;
use codex_context_checkpoint::MemoryStatusSnapshot;
use codex_protocol::models::ResponseItem;

const MAX_SETTLEABLE_GROUPS: usize = 16;
const MAX_DEGRADED_REASON_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckpointStatusFragment {
    status: MemoryStatusSnapshot,
}

impl CheckpointStatusFragment {
    pub(crate) fn new(status: MemoryStatusSnapshot) -> Self {
        Self { status }
    }

    pub(crate) fn tool_result_share_basis_points(input: &[ResponseItem]) -> u16 {
        let mut total_bytes = 0_u64;
        let mut tool_result_bytes = 0_u64;
        for item in input {
            let bytes = serde_json::to_vec(item)
                .map(|serialized| serialized.len() as u64)
                .unwrap_or(0);
            total_bytes = total_bytes.saturating_add(bytes);
            if matches!(
                item,
                ResponseItem::FunctionCallOutput { .. }
                    | ResponseItem::CustomToolCallOutput { .. }
                    | ResponseItem::ToolSearchOutput { .. }
            ) {
                tool_result_bytes = tool_result_bytes.saturating_add(bytes);
            }
        }
        if total_bytes == 0 {
            return 0;
        }
        tool_result_bytes
            .saturating_mul(10_000)
            .checked_div(total_bytes)
            .unwrap_or(0)
            .min(10_000) as u16
    }
}

impl ContextualUserFragment for CheckpointStatusFragment {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<MEMORY_STATUS>\n", "\n</MEMORY_STATUS>")
    }

    fn body(&self) -> String {
        let settleable = self
            .status
            .settleable_group_ids
            .iter()
            .take(MAX_SETTLEABLE_GROUPS)
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let omitted = self
            .status
            .settleable_group_ids
            .len()
            .saturating_sub(MAX_SETTLEABLE_GROUPS);
        let settleable = if omitted == 0 {
            settleable
        } else {
            format!("{settleable},+{omitted}")
        };
        let checkpoint_required = matches!(
            self.status.pressure,
            CheckpointPressure::Required | CheckpointPressure::FallbackRequired
        );
        let degraded = self.status.degraded_reason.as_deref().map(|reason| {
            let mut bounded = reason
                .chars()
                .take(MAX_DEGRADED_REASON_CHARS)
                .collect::<String>();
            if reason.chars().count() > MAX_DEGRADED_REASON_CHARS {
                bounded.push_str("...");
            }
            bounded
        });
        format!(
            "generation={} turn={}\ncompleted_sessions={} quarter_consolidation_due={}\n\
             context_usage={} usage_source={:?} tool_result_share={}\n\
             tool_groups={} open={} settled={} settleable=[{}]\nlast_checkpoint={}\n\
             pressure={:?} checkpoint_required={} tool=update_context_state recall=recall_checkpoint_artifact\n\
             policy=retain_knowledge+retain_uncertainty+archive_process;no_next_action;settle_each_group{}",
            self.status.generation_id,
            self.status.turn_id,
            self.status.completed_sessions,
            self.status.quarter_consolidation_due,
            format_basis_points(self.status.context_usage_basis_points),
            self.status.usage_source,
            format_basis_points(self.status.tool_result_share_basis_points),
            self.status.open_groups + self.status.settled_groups,
            self.status.open_groups,
            self.status.settled_groups,
            settleable,
            self.status
                .last_checkpoint
                .map(|checkpoint| checkpoint.to_string())
                .unwrap_or_else(|| "none".to_string()),
            self.status.pressure,
            checkpoint_required,
            degraded
                .map(|reason| format!("\ndegraded={reason}"))
                .unwrap_or_default(),
        )
    }
}

fn format_basis_points(basis_points: u16) -> String {
    format!("{}.{:02}%", basis_points / 100, basis_points % 100)
}

#[cfg(test)]
#[path = "checkpoint_status_tests.rs"]
mod tests;
