use super::ContextualUserFragment;
use crate::compact::SUMMARY_PREFIX;
use codex_context_checkpoint::CheckpointError;
use codex_context_checkpoint::TurnRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckpointRecordFragment {
    body: String,
}

impl CheckpointRecordFragment {
    pub(crate) fn new(record: &TurnRecord) -> Result<Self, CheckpointError> {
        let record = serde_json::to_string(record)
            .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?;
        Ok(Self {
            body: format!("{SUMMARY_PREFIX}\n{record}"),
        })
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
