use super::ContextualUserFragment;
use crate::state::WorkflowRuntimeState;

pub(crate) struct WorkflowRuntimeFragment {
    body: String,
}

impl WorkflowRuntimeFragment {
    pub(crate) fn new(state: &WorkflowRuntimeState) -> Self {
        Self {
            body: state.model_view(),
        }
    }

    pub(crate) fn matches_revision(text: &str, revision: u64) -> bool {
        text.contains(format!("revision: {revision}").as_str())
    }
}

impl ContextualUserFragment for WorkflowRuntimeFragment {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<WORKFLOW_RUNTIME_STATE>\n", "\n</WORKFLOW_RUNTIME_STATE>")
    }

    fn body(&self) -> String {
        self.body.clone()
    }
}
