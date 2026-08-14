use crate::model::CheckpointError;
use crate::model::EvidenceRef;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

const MAX_ACTIVE_ENTRIES: usize = 24;
const MAX_LAYER_ENTRIES: usize = 64;
const MAX_LIST_ITEMS: usize = 24;
const MAX_KEY_BYTES: usize = 128;
const MAX_CONTENT_BYTES: usize = 4 * 1024;
const MAX_ACTIVE_TEXT_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateEntryKind {
    SourceFact,
    UserDecision,
    Constraint,
    Validation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEntry {
    pub key: String,
    pub kind: StateEntryKind,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_revision: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContextState {
    pub objective: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<StateEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_questions: Vec<String>,
    pub next_action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStateSnapshot {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quarter: Vec<StateEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session: Vec<StateEntry>,
    pub active: ActiveContextState,
}

impl ContextStateSnapshot {
    pub fn is_empty(&self) -> bool {
        self.quarter.is_empty()
            && self.session.is_empty()
            && self.active == ActiveContextState::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStateUpdate {
    pub active: ActiveContextState,
    #[serde(default)]
    pub session_upserts: Vec<StateEntry>,
    #[serde(default)]
    pub forget_keys: Vec<String>,
    #[serde(default)]
    pub quarter_promotions: Vec<String>,
}

pub(crate) fn apply_context_state_update(
    current: &ContextStateSnapshot,
    update: &ContextStateUpdate,
) -> Result<ContextStateSnapshot, CheckpointError> {
    validate_active_state(&update.active)?;
    validate_entries("sessionUpserts", &update.session_upserts, MAX_LAYER_ENTRIES)?;
    validate_keys("forgetKeys", &update.forget_keys)?;
    validate_keys("quarterPromotions", &update.quarter_promotions)?;

    let mut quarter = entries_by_key("quarter", &current.quarter)?;
    let mut session = entries_by_key("session", &current.session)?;

    for key in &update.forget_keys {
        quarter.remove(key);
        session.remove(key);
    }

    for key in &update.quarter_promotions {
        let Some(entry) = session.remove(key) else {
            return Err(CheckpointError::InvalidRequest(format!(
                "quarter promotion {key:?} must reference existing session memory"
            )));
        };
        quarter.insert(key.clone(), entry);
    }

    for entry in &update.session_upserts {
        if quarter.contains_key(&entry.key) {
            return Err(CheckpointError::InvalidRequest(format!(
                "session entry {:?} conflicts with quarter memory; forget it before replacing it",
                entry.key
            )));
        }
        session.insert(entry.key.clone(), entry.clone());
    }

    if quarter.len() > MAX_LAYER_ENTRIES || session.len() > MAX_LAYER_ENTRIES {
        return Err(CheckpointError::InvalidRequest(format!(
            "context memory layer exceeds {MAX_LAYER_ENTRIES} entries"
        )));
    }

    for entry in &update.active.entries {
        if quarter.contains_key(&entry.key) || session.contains_key(&entry.key) {
            return Err(CheckpointError::InvalidRequest(format!(
                "active entry {:?} duplicates existing memory",
                entry.key
            )));
        }
    }

    Ok(ContextStateSnapshot {
        quarter: quarter.into_values().collect(),
        session: session.into_values().collect(),
        active: update.active.clone(),
    })
}

pub(crate) fn update_evidence(update: &ContextStateUpdate) -> Vec<EvidenceRef> {
    update
        .active
        .entries
        .iter()
        .chain(&update.session_upserts)
        .flat_map(|entry| entry.evidence.iter().cloned())
        .collect()
}

fn validate_active_state(active: &ActiveContextState) -> Result<(), CheckpointError> {
    validate_required_text("active.objective", &active.objective, MAX_ACTIVE_TEXT_BYTES)?;
    validate_required_text(
        "active.nextAction",
        &active.next_action,
        MAX_ACTIVE_TEXT_BYTES,
    )?;
    validate_entries("active.entries", &active.entries, MAX_ACTIVE_ENTRIES)?;
    validate_text_list("active.constraints", &active.constraints)?;
    validate_text_list("active.openQuestions", &active.open_questions)
}

fn validate_entries(
    field: &str,
    entries: &[StateEntry],
    maximum: usize,
) -> Result<(), CheckpointError> {
    if entries.len() > maximum {
        return Err(CheckpointError::InvalidRequest(format!(
            "{field} has {} entries; maximum is {maximum}",
            entries.len()
        )));
    }

    let mut keys = BTreeSet::new();
    for entry in entries {
        validate_required_text("state entry key", &entry.key, MAX_KEY_BYTES)?;
        validate_required_text("state entry content", &entry.content, MAX_CONTENT_BYTES)?;
        if !keys.insert(entry.key.as_str()) {
            return Err(CheckpointError::InvalidRequest(format!(
                "{field} contains duplicate key {:?}",
                entry.key
            )));
        }
        if matches!(
            entry.kind,
            StateEntryKind::SourceFact | StateEntryKind::Validation
        ) && entry.evidence.is_empty()
        {
            return Err(CheckpointError::InvalidRequest(format!(
                "state entry {:?} requires evidence",
                entry.key
            )));
        }
        if entry
            .source_revision
            .as_ref()
            .is_some_and(|revision| revision.trim().is_empty())
        {
            return Err(CheckpointError::InvalidRequest(format!(
                "state entry {:?} has an empty source revision",
                entry.key
            )));
        }
    }
    Ok(())
}

fn validate_keys(field: &str, keys: &[String]) -> Result<(), CheckpointError> {
    if keys.len() > MAX_LAYER_ENTRIES {
        return Err(CheckpointError::InvalidRequest(format!(
            "{field} has {} keys; maximum is {MAX_LAYER_ENTRIES}",
            keys.len()
        )));
    }
    let mut unique = BTreeSet::new();
    for key in keys {
        validate_required_text(field, key, MAX_KEY_BYTES)?;
        if !unique.insert(key.as_str()) {
            return Err(CheckpointError::InvalidRequest(format!(
                "{field} contains duplicate key {key:?}"
            )));
        }
    }
    Ok(())
}

fn validate_text_list(field: &str, values: &[String]) -> Result<(), CheckpointError> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(CheckpointError::InvalidRequest(format!(
            "{field} has {} items; maximum is {MAX_LIST_ITEMS}",
            values.len()
        )));
    }
    for value in values {
        validate_required_text(field, value, MAX_ACTIVE_TEXT_BYTES)?;
    }
    Ok(())
}

fn validate_required_text(
    field: &str,
    value: &str,
    maximum_bytes: usize,
) -> Result<(), CheckpointError> {
    if value.trim().is_empty() {
        return Err(CheckpointError::InvalidRequest(format!(
            "{field} must not be empty"
        )));
    }
    if value.len() > maximum_bytes {
        return Err(CheckpointError::InvalidRequest(format!(
            "{field} is {} bytes; maximum is {maximum_bytes}",
            value.len()
        )));
    }
    Ok(())
}

fn entries_by_key(
    field: &str,
    entries: &[StateEntry],
) -> Result<BTreeMap<String, StateEntry>, CheckpointError> {
    validate_entries(field, entries, MAX_LAYER_ENTRIES)?;
    Ok(entries
        .iter()
        .cloned()
        .map(|entry| (entry.key.clone(), entry))
        .collect())
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
