// CTF|认证 - Bounded task-relevant projection for durable project research.

use crate::ResearchEntry;
use crate::ResearchEntryKind;
use crate::ResearchScope;
use crate::ResearchStatus;
use std::collections::BTreeSet;

const RESEARCH_CONTEXT_SCHEMA_VERSION: u16 = 1;
const RESEARCH_CONTEXT_MAX_ENTRIES: usize = 8;
const RESEARCH_CONTEXT_MAX_PROJECTED_BYTES: usize = 2_600;
const MAX_SUBJECT_BYTES: usize = 160;
const MAX_STATEMENT_BYTES: usize = 640;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ResearchContextCacheKey {
    project_id: String,
    project_revision: u64,
    task_signature: String,
    schema_version: u16,
}

impl ResearchContextCacheKey {
    pub fn new(project_id: String, project_revision: u64, task_signature: String) -> Self {
        Self {
            project_id,
            project_revision,
            task_signature,
            schema_version: RESEARCH_CONTEXT_SCHEMA_VERSION,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResearchContextProjection {
    pub entries: Vec<ResearchEntry>,
    pub omitted_count: usize,
}

pub fn project_research_context(
    entries: &[ResearchEntry],
    task_text: &str,
) -> ResearchContextProjection {
    let task_terms = tokenize(task_text);
    let task_lower = task_text.to_lowercase();
    let mut candidates = entries
        .iter()
        .filter(|entry| entry.scope == ResearchScope::Project)
        .cloned()
        .filter_map(|entry| {
            relevance_score(&entry, &task_terms, task_lower.as_str()).map(|score| (score, entry))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.id.cmp(&right.id))
    });

    let candidate_count = candidates.len();
    let mut projected_bytes: usize = 0;
    let mut projected = Vec::new();
    for (_, mut entry) in candidates {
        entry.subject = truncate_utf8(entry.subject.as_str(), MAX_SUBJECT_BYTES);
        entry.statement = truncate_utf8(entry.statement.as_str(), MAX_STATEMENT_BYTES);
        let entry_bytes = entry.subject.len() + entry.statement.len() + entry.id.len() + 96;
        if projected.len() >= RESEARCH_CONTEXT_MAX_ENTRIES
            || projected_bytes.saturating_add(entry_bytes) > RESEARCH_CONTEXT_MAX_PROJECTED_BYTES
        {
            continue;
        }
        projected_bytes = projected_bytes.saturating_add(entry_bytes);
        projected.push(entry);
    }

    ResearchContextProjection {
        omitted_count: candidate_count.saturating_sub(projected.len()),
        entries: projected,
    }
}

fn relevance_score(
    entry: &ResearchEntry,
    task_terms: &BTreeSet<String>,
    task_lower: &str,
) -> Option<i64> {
    let entry_terms =
        tokenize(format!("{} {} {}", entry.id, entry.subject, entry.statement).as_str());
    let overlapping_terms = task_terms.intersection(&entry_terms).collect::<Vec<_>>();
    let overlap = overlapping_terms.len() as i64;
    let subject_lower = entry.subject.to_lowercase();
    let subject_match = !task_lower.trim().is_empty()
        && subject_lower.chars().count() >= 3
        && (task_lower.contains(subject_lower.as_str()) || subject_lower.contains(task_lower));
    let term_match = overlapping_terms.iter().any(|term| term.is_ascii()) || overlap >= 2;
    if !subject_match && !term_match {
        return None;
    }
    let subject_score = if subject_match { 80 } else { 0 };

    Some(overlap * 100 + subject_score + kind_weight(entry.kind) + status_weight(entry.status))
}

fn kind_weight(kind: ResearchEntryKind) -> i64 {
    match kind {
        ResearchEntryKind::Constraint => 50,
        ResearchEntryKind::Goal => 46,
        ResearchEntryKind::Decision => 44,
        ResearchEntryKind::Fact => 40,
        ResearchEntryKind::Unknown => 36,
        ResearchEntryKind::FuturePressure => 32,
        ResearchEntryKind::Hypothesis => 28,
        ResearchEntryKind::Exploration => 24,
        ResearchEntryKind::Outcome => 20,
    }
}

fn status_weight(status: ResearchStatus) -> i64 {
    match status {
        ResearchStatus::Supported => 20,
        ResearchStatus::Open => 12,
        ResearchStatus::Resolved => 8,
        ResearchStatus::Rejected => 0,
        ResearchStatus::Superseded => -12,
    }
}

fn tokenize(text: &str) -> BTreeSet<String> {
    let mut terms = BTreeSet::new();
    let mut ascii = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            ascii.push(character.to_ascii_lowercase());
            continue;
        }
        if ascii.len() >= 2 {
            terms.insert(std::mem::take(&mut ascii));
        } else {
            ascii.clear();
        }
        if character.is_alphanumeric() {
            terms.insert(character.to_lowercase().collect());
        }
    }
    if ascii.len() >= 2 {
        terms.insert(ascii);
    }
    terms
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let content_limit = max_bytes.saturating_sub('…'.len_utf8());
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= content_limit)
        .last()
        .unwrap_or(0);
    format!("{}…", &text[..end])
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
