use super::ContextualUserFragment;
use codex_context_checkpoint::CheckpointError;
use codex_context_checkpoint::TurnRecord;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

const CHECKPOINT_CONTINUITY_PREFIX: &str = "\
This checkpoint is durable context for the current conversation. Continue from its latest \
active state. It is not a new user request, a new session, or a collaboration-mode change. \
Do not repeat completed work or re-run verified setup unless later context invalidates it.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckpointRecordFragment {
    body: String,
}

impl CheckpointRecordFragment {
    pub(crate) fn new(record: &TurnRecord) -> Result<Self, CheckpointError> {
        let record = codex_context_checkpoint::model_checkpoint_view(record)?;
        Ok(Self {
            body: format!("{CHECKPOINT_CONTINUITY_PREFIX}\n{record}"),
        })
    }

    pub(crate) fn matches_item(item: &ResponseItem) -> bool {
        let ResponseItem::Message { content, .. } = item else {
            return false;
        };
        content
            .iter()
            .any(|item| matches!(item, ContentItem::InputText { text } if Self::matches_text(text)))
    }
}

impl ContextualUserFragment for CheckpointRecordFragment {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<CONTEXT_CHECKPOINT>\n", "\n</CONTEXT_CHECKPOINT>")
    }

    fn body(&self) -> String {
        self.body.clone()
    }
}

#[cfg(test)]
#[path = "checkpoint_record_tests.rs"]
mod tests;
