use crate::ArtifactId;
use crate::CheckpointError;
use crate::CheckpointGenerationId;
use crate::EvidenceRef;
use crate::StateEntry;
use crate::StateEntryKind;
use crate::StateRemoval;
use crate::StateRemovalReason;
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

pub fn model_checkpoint_view(record: &TurnRecord) -> Result<String, CheckpointError> {
    let artifacts = artifact_catalog(record);
    let mut lines = vec![format!(
        "Checkpoint {}",
        checkpoint_reference(record.generation_id, record.record_id)
    )];

    append_state_layer(
        &mut lines,
        "Quarter",
        &record.state.quarter,
        record.record_id,
        &artifacts,
    );
    append_state_layer(
        &mut lines,
        "Session",
        &record.state.session,
        record.record_id,
        &artifacts,
    );
    append_active_state(&mut lines, record, &artifacts);
    append_legacy_state(&mut lines, record, &artifacts);
    append_state_removals(&mut lines, &record.state_removals);

    let view = lines.join("\n");
    if view.len() > MAX_MODEL_CHECKPOINT_BYTES {
        return Err(CheckpointError::InvalidRequest(format!(
            "model checkpoint view is {} bytes; maximum is {MAX_MODEL_CHECKPOINT_BYTES}",
            view.len()
        )));
    }
    Ok(view)
}

fn append_state_layer(
    lines: &mut Vec<String>,
    title: &str,
    entries: &[StateEntry],
    record_id: TurnRecordId,
    artifacts: &[&ArtifactId],
) {
    if entries.is_empty() {
        return;
    }
    push_section(lines, title);
    append_projected_entries(lines, project_entries(entries, record_id, artifacts), 2);
}

fn append_active_state(lines: &mut Vec<String>, record: &TurnRecord, artifacts: &[&ArtifactId]) {
    let active = &record.state.active;
    if active.objective.is_empty()
        && active.entries.is_empty()
        && active.constraints.is_empty()
        && active.open_questions.is_empty()
        && active.continuity_hints.is_empty()
    {
        return;
    }

    push_section(lines, "Active");
    append_named_text(lines, "Objective", &active.objective, 2);
    append_projected_entries(
        lines,
        project_entries(&active.entries, record.record_id, artifacts),
        2,
    );
    append_named_list(lines, "Constraints", &active.constraints, 2);
    append_named_list(lines, "Open", &active.open_questions, 2);
    append_named_list(lines, "May matter later", &active.continuity_hints, 2);
}

fn append_legacy_state(lines: &mut Vec<String>, record: &TurnRecord, artifacts: &[&ArtifactId]) {
    if !record.summary.is_empty() {
        push_section(lines, "Summary");
        append_text(lines, &record.summary, 2);
    }
    append_top_level_list(lines, "Changes", &record.changes);
    append_top_level_list(lines, "Validation", &record.validation);
    append_top_level_list(lines, "Decisions", &record.decisions);
    append_top_level_list(lines, "Open items", &record.open_items);

    if !record.evidence.is_empty() {
        let mut evidence = record
            .evidence
            .iter()
            .map(|reference| evidence_reference(reference, record.record_id, artifacts))
            .collect::<Vec<_>>();
        evidence.sort();
        evidence.dedup();
        push_section(lines, "Evidence");
        append_bullets(lines, &evidence, 2);
    }
}

fn append_state_removals(lines: &mut Vec<String>, removals: &[StateRemoval]) {
    if removals.is_empty() {
        return;
    }
    let mut rendered = removals
        .iter()
        .map(|removal| match removal.reason {
            StateRemovalReason::UserRevoked => {
                format!("{} — user revoked", removal.key)
            }
            StateRemovalReason::Superseded => format!(
                "{} — superseded by {}",
                removal.key,
                removal.superseded_by.as_deref().unwrap_or("unknown")
            ),
        })
        .collect::<Vec<_>>();
    rendered.sort();
    push_section(lines, "Retired");
    append_bullets(lines, &rendered, 2);
}

fn append_projected_entries(
    lines: &mut Vec<String>,
    entries: Vec<ModelStateEntry<'_>>,
    indent: usize,
) {
    let prefix = " ".repeat(indent);
    let detail_prefix = " ".repeat(indent + 2);
    for entry in entries {
        lines.push(format!("{prefix}{}", entry.reference));
        append_text(lines, entry.content, indent + 2);
        if !entry.evidence_refs.is_empty() {
            lines.push(format!(
                "{detail_prefix}evidence: {}",
                entry.evidence_refs.join(" · ")
            ));
        }
        if let Some(status) = entry.source_status {
            lines.push(format!("{detail_prefix}source: {status}"));
        }
    }
}

fn append_named_text(lines: &mut Vec<String>, title: &str, value: &str, indent: usize) {
    if value.is_empty() {
        return;
    }
    lines.push(format!("{}{title}", " ".repeat(indent)));
    append_text(lines, value, indent + 2);
}

fn append_named_list(lines: &mut Vec<String>, title: &str, values: &[String], indent: usize) {
    if values.is_empty() {
        return;
    }
    lines.push(format!("{}{title}", " ".repeat(indent)));
    append_bullets(lines, values, indent + 2);
}

fn append_top_level_list(lines: &mut Vec<String>, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    push_section(lines, title);
    append_bullets(lines, values, 2);
}

fn append_bullets(lines: &mut Vec<String>, values: &[String], indent: usize) {
    let first_prefix = format!("{}- ", " ".repeat(indent));
    let continuation_prefix = " ".repeat(indent + 2);
    for value in values {
        let mut value_lines = value.lines();
        if let Some(first) = value_lines.next() {
            lines.push(format!("{first_prefix}{first}"));
            lines.extend(value_lines.map(|line| format!("{continuation_prefix}{line}")));
        }
    }
}

fn append_text(lines: &mut Vec<String>, value: &str, indent: usize) {
    let prefix = " ".repeat(indent);
    lines.extend(value.lines().map(|line| format!("{prefix}{line}")));
}

fn push_section(lines: &mut Vec<String>, title: &str) {
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.push(title.to_string());
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
