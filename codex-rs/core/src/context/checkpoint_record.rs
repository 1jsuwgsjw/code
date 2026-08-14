use super::ContextualUserFragment;
use crate::compact::SUMMARY_PREFIX;
use codex_context_checkpoint::CheckpointError;
use codex_context_checkpoint::TurnRecord;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckpointRecordFragment {
    body: String,
}

impl CheckpointRecordFragment {
    pub(crate) fn new(record: &TurnRecord) -> Result<Self, CheckpointError> {
        let record = codex_context_checkpoint::model_checkpoint_json(record)?;
        Ok(Self {
            body: format!("{SUMMARY_PREFIX}\n{record}"),
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
        "user"
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
