use crate::CheckpointPressure;
use crate::ContextUsageSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointBudget {
    pub final_answer_reserve_tokens: u64,
    pub checkpoint_reserve_tokens: u64,
    pub provider_overhead_tokens: u64,
    pub advisory_basis_points: u16,
    pub required_basis_points: u16,
    pub fallback_basis_points: u16,
}

impl Default for CheckpointBudget {
    fn default() -> Self {
        Self {
            final_answer_reserve_tokens: 4_096,
            checkpoint_reserve_tokens: 2_048,
            provider_overhead_tokens: 1_024,
            advisory_basis_points: 7_500,
            required_basis_points: 8_500,
            fallback_basis_points: 9_500,
        }
    }
}

impl CheckpointBudget {
    pub fn usage_basis_points(&self, usage: ContextUsageSnapshot) -> u16 {
        let Some(context_window) = usage.context_window else {
            return 0;
        };
        let reserve = self
            .final_answer_reserve_tokens
            .saturating_add(self.checkpoint_reserve_tokens)
            .saturating_add(self.provider_overhead_tokens);
        let usable = context_window.saturating_sub(reserve).max(1);
        usage
            .active_tokens
            .saturating_mul(10_000)
            .checked_div(usable)
            .unwrap_or(10_000)
            .min(10_000) as u16
    }

    pub fn pressure(&self, usage_basis_points: u16) -> CheckpointPressure {
        if usage_basis_points >= self.fallback_basis_points {
            CheckpointPressure::FallbackRequired
        } else if usage_basis_points >= self.required_basis_points {
            CheckpointPressure::Required
        } else if usage_basis_points >= self.advisory_basis_points {
            CheckpointPressure::Advisory
        } else {
            CheckpointPressure::Normal
        }
    }
}
