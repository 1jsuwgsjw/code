use super::PreviousSectionState;
use super::WorldStateSection;
use crate::context::ContextualUserFragment;
use crate::context::WorkflowRuntimeFragment;
use crate::state::WorkflowRuntimeState;

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkflowRuntimeWorldState {
    state: WorkflowRuntimeState,
}

impl WorkflowRuntimeWorldState {
    pub(crate) fn new(state: WorkflowRuntimeState) -> Self {
        Self { state }
    }
}

impl WorldStateSection for WorkflowRuntimeWorldState {
    const ID: &'static str = crate::state::WORKFLOW_RUNTIME_WORLD_STATE_ID;
    type Snapshot = WorkflowRuntimeState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state.clone()
    }

    fn has_retained_fragment_matcher(&self) -> bool {
        true
    }

    fn matches_retained_fragment(&self, role: &str, text: &str) -> bool {
        role == "developer"
            && WorkflowRuntimeFragment::matches_revision(text, self.state.task_revision())
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        if matches!(previous, PreviousSectionState::Known(previous) if previous == &self.state) {
            return None;
        }
        Some(Box::new(WorkflowRuntimeFragment::new(&self.state)))
    }
}
