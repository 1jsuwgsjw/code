//! Domain model and reducer for Codex research state.
//!
//! Research state records project- and task-scoped facts, unknowns, hypotheses,
//! explorations, decisions, and outcomes without coupling the model to a
//! domain-specific schema.

pub use codex_protocol::plan_tool::ResearchDelta;
pub use codex_protocol::plan_tool::ResearchEntryKind;
pub use codex_protocol::plan_tool::ResearchOperation;
pub use codex_protocol::plan_tool::ResearchScope;
pub use codex_protocol::plan_tool::ResearchStatus;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use thiserror::Error;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResearchEntry {
    pub id: String,
    pub scope: ResearchScope,
    pub kind: ResearchEntryKind,
    pub subject: String,
    pub statement: String,
    pub status: ResearchStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResearchEntryKey {
    scope: ResearchScope,
    id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResearchState {
    revision: u64,
    entries: BTreeMap<ResearchEntryKey, ResearchEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchApplyResult {
    pub revision: u64,
    pub changed: bool,
    pub entries: Vec<ResearchEntry>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResearchStateError {
    #[error("research entry id must not be empty")]
    EmptyId,
    #[error("research upsert `{id}` requires field `{field}`")]
    MissingUpsertField { id: String, field: &'static str },
    #[error("research status update `{id}` requires field `status`")]
    MissingStatus { id: String },
    #[error("research entry `{id}` does not exist in scope `{scope:?}`")]
    EntryNotFound { id: String, scope: ResearchScope },
}

impl ResearchState {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entries(&self) -> Vec<ResearchEntry> {
        self.entries.values().cloned().collect()
    }

    pub fn apply(
        &mut self,
        deltas: &[ResearchDelta],
    ) -> Result<ResearchApplyResult, ResearchStateError> {
        let mut next_entries = self.entries.clone();
        for delta in deltas {
            apply_delta(&mut next_entries, delta)?;
        }

        let changed = next_entries != self.entries;
        if changed {
            self.entries = next_entries;
            self.revision = self.revision.saturating_add(1);
        }

        Ok(ResearchApplyResult {
            revision: self.revision,
            changed,
            entries: self.entries(),
        })
    }
}

fn apply_delta(
    entries: &mut BTreeMap<ResearchEntryKey, ResearchEntry>,
    delta: &ResearchDelta,
) -> Result<(), ResearchStateError> {
    let id = delta.id.trim();
    if id.is_empty() {
        return Err(ResearchStateError::EmptyId);
    }
    let key = ResearchEntryKey {
        scope: delta.scope,
        id: id.to_string(),
    };

    match delta.operation {
        ResearchOperation::Upsert => {
            let kind = required_upsert_field(delta.kind, id, "kind")?;
            let subject = required_upsert_text(delta.subject.as_deref(), id, "subject")?;
            let statement = required_upsert_text(delta.statement.as_deref(), id, "statement")?;
            let entry = ResearchEntry {
                id: id.to_string(),
                scope: delta.scope,
                kind,
                subject,
                statement,
                status: delta.status.unwrap_or(ResearchStatus::Open),
            };
            entries.insert(key, entry);
        }
        ResearchOperation::SetStatus => {
            let status = delta
                .status
                .ok_or_else(|| ResearchStateError::MissingStatus { id: id.to_string() })?;
            let entry = entries
                .get_mut(&key)
                .ok_or_else(|| ResearchStateError::EntryNotFound {
                    id: id.to_string(),
                    scope: delta.scope,
                })?;
            entry.status = status;
        }
        ResearchOperation::Remove => {
            entries.remove(&key);
        }
    }

    Ok(())
}

fn required_upsert_field<T: Copy>(
    value: Option<T>,
    id: &str,
    field: &'static str,
) -> Result<T, ResearchStateError> {
    value.ok_or_else(|| ResearchStateError::MissingUpsertField {
        id: id.to_string(),
        field,
    })
}

fn required_upsert_text(
    value: Option<&str>,
    id: &str,
    field: &'static str,
) -> Result<String, ResearchStateError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ResearchStateError::MissingUpsertField {
            id: id.to_string(),
            field,
        })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
