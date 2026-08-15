use crate::ArtifactId;
use crate::CheckpointError;
use crate::CheckpointGenerationId;
use crate::EvidenceRef;
use crate::StateEntry;
use crate::StateEntryKind;
use crate::StateRemoval;
use crate::TurnRecord;
use crate::TurnRecordId;
use serde::Serialize;
use std::collections::BTreeSet;
use std::collections::HashSet;

const MAX_MODEL_CHECKPOINT_BYTES: usize = 40 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelCheckpoint<'a> {
    checkpoint_ref: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    quarter: Vec<ModelStateEntry<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    session: Vec<ModelStateEntry<'a>>,
    active: ModelActiveState<'a>,
    #[serde(skip_serializing_if = "str::is_empty")]
    summary: &'a str,
    #[serde(skip_serializing_if = "slice_is_empty")]
    changes: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    validation: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    decisions: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    open_items: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    state_removals: &'a [StateRemoval],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    recall_refs: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelActiveState<'a> {
    objective: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entries: Vec<ModelStateEntry<'a>>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    constraints: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    open_questions: &'a [String],
    #[serde(skip_serializing_if = "slice_is_empty")]
    continuity_hints: &'a [String],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStateEntry<'a> {
    reference: String,
    content: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_status: Option<&'static str>,
}

pub fn checkpoint_reference(
    generation_id: CheckpointGenerationId,
    record_id: TurnRecordId,
) -> String {
    format!("ctx:{generation_id}/{record_id}")
}

pub fn checkpoint_labels(record: &TurnRecord) -> Vec<String> {
    let mut labels = BTreeSet::new();
    if !record.state.quarter.is_empty() {
        labels.insert("quarter");
    }
    if !record.state.session.is_empty() {
        labels.insert("session");
    }
    if !record.state.active.open_questions.is_empty() || !record.open_items.is_empty() {
        labels.insert("open");
    }
    if !record.state.active.continuity_hints.is_empty() {
        labels.insert("continuity");
    }
    if !record.state_removals.is_empty() {
        labels.insert("superseded");
    }
    if !record.summary.is_empty() {
        labels.insert("summary");
    }
    if !record.decisions.is_empty() {
        labels.insert("decision");
    }
    if !record.validation.is_empty() {
        labels.insert("validation");
    }
    for entry in state_entries(record) {
        labels.insert(match entry.kind {
            StateEntryKind::SourceFact => "source",
            StateEntryKind::SourceCoverage => "coverage",
            StateEntryKind::UserDecision => "decision",
            StateEntryKind::Constraint => "constraint",
            StateEntryKind::Validation => "validation",
        });
    }
    labels.into_iter().map(str::to_string).collect()
}

pub fn checkpoint_state_entry_count(record: &TurnRecord) -> usize {
    state_entries(record).count()
}

pub fn model_checkpoint_json(record: &TurnRecord) -> Result<String, CheckpointError> {
    let artifacts = artifact_catalog(record);
    let projection = ModelCheckpoint {
        checkpoint_ref: checkpoint_reference(record.generation_id, record.record_id),
        quarter: project_entries(&record.state.quarter, record.record_id, &artifacts),
        session: project_entries(&record.state.session, record.record_id, &artifacts),
        active: ModelActiveState {
            objective: &record.state.active.objective,
            entries: project_entries(&record.state.active.entries, record.record_id, &artifacts),
            constraints: &record.state.active.constraints,
            open_questions: &record.state.active.open_questions,
            continuity_hints: &record.state.active.continuity_hints,
        },
        summary: &record.summary,
        changes: &record.changes,
        validation: &record.validation,
        decisions: &record.decisions,
        open_items: &record.open_items,
        state_removals: &record.state_removals,
        recall_refs: (0..artifacts.len())
            .map(|index| artifact_reference(record.record_id, index))
            .collect(),
    };
    let serialized = serde_json::to_string(&projection)
        .map_err(|error| CheckpointError::InvalidRequest(error.to_string()))?;
    if serialized.len() > MAX_MODEL_CHECKPOINT_BYTES {
        return Err(CheckpointError::InvalidRequest(format!(
            "model checkpoint projection is {} bytes; maximum is {MAX_MODEL_CHECKPOINT_BYTES}",
            serialized.len()
        )));
    }
    Ok(serialized)
}

pub(crate) fn artifact_id_for_reference<'a>(
    record: &'a TurnRecord,
    reference: &str,
) -> Option<&'a ArtifactId> {
    let suffix = reference.strip_prefix("artifact:")?;
    let (record_id, artifact_index) = suffix.split_once("/A")?;
    if record_id != record.record_id.to_string() {
        return None;
    }
    let artifact_index = artifact_index.parse::<usize>().ok()?.checked_sub(1)?;
    artifact_catalog(record).get(artifact_index).copied()
}

fn slice_is_empty<T>(value: &[T]) -> bool {
    value.is_empty()
}

fn state_entries(record: &TurnRecord) -> impl Iterator<Item = &StateEntry> {
    record
        .state
        .quarter
        .iter()
        .chain(&record.state.session)
        .chain(&record.state.active.entries)
}

fn artifact_catalog(record: &TurnRecord) -> Vec<&ArtifactId> {
    let mut seen = HashSet::new();
    let mut artifacts = Vec::new();
    let evidence = state_entries(record)
        .flat_map(|entry| &entry.evidence)
        .chain(&record.evidence);
    for reference in evidence {
        if let EvidenceRef::Artifact { artifact_id } = reference
            && seen.insert(artifact_id.clone())
        {
            artifacts.push(artifact_id);
        }
    }
    artifacts
}

fn project_entries<'a>(
    entries: &'a [StateEntry],
    record_id: TurnRecordId,
    artifacts: &[&ArtifactId],
) -> Vec<ModelStateEntry<'a>> {
    let mut projected = entries
        .iter()
        .map(|entry| ModelStateEntry {
            reference: state_entry_reference(entry),
            content: &entry.content,
            evidence_refs: entry
                .evidence
                .iter()
                .map(|reference| evidence_reference(reference, record_id, artifacts))
                .collect(),
            source_status: entry.source_revision.as_ref().map(|_| "verified"),
        })
        .collect::<Vec<_>>();
    projected.sort_by(|left, right| left.reference.cmp(&right.reference));
    projected
}

fn state_entry_reference(entry: &StateEntry) -> String {
    let category = match entry.kind {
        StateEntryKind::SourceFact => "fact:source",
        StateEntryKind::SourceCoverage => "coverage:source",
        StateEntryKind::UserDecision => "decision",
        StateEntryKind::Constraint => "constraint",
        StateEntryKind::Validation => "validation",
    };
    format!("{category}/{}", entry.key)
}

fn evidence_reference(
    reference: &EvidenceRef,
    record_id: TurnRecordId,
    artifacts: &[&ArtifactId],
) -> String {
    match reference {
        EvidenceRef::Artifact { artifact_id } => artifacts
            .iter()
            .position(|candidate| *candidate == artifact_id)
            .map(|index| artifact_reference(record_id, index))
            .unwrap_or_else(|| "artifact:unavailable".to_string()),
        EvidenceRef::WorkspacePath { path } => format!("workspace:{path}"),
        EvidenceRef::Symbol { value } => format!("symbol:{value}"),
    }
}

fn artifact_reference(record_id: TurnRecordId, index: usize) -> String {
    let display_index = index + 1;
    format!("artifact:{record_id}/A{display_index:03}")
}

#[cfg(test)]
#[path = "model_projection_tests.rs"]
mod tests;
