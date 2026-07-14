// CTF|认证 - Bounded project research context shown to the model.

use super::ContextualUserFragment;
use codex_research_state::ResearchContextProjection;
use codex_research_state::ResearchEntryKind;
use codex_research_state::ResearchStatus;

const OPEN_MARKER: &str = "<research_context>\n";
const CLOSE_MARKER: &str = "\n</research_context>";
const MAX_PROJECTED_CONTENT_BYTES: usize = 3_000;
const MAX_RENDERED_SUBJECT_BYTES: usize = 240;
const MAX_RENDERED_STATEMENT_BYTES: usize = 800;

pub(crate) struct ResearchContextFragment {
    body: String,
}

impl ResearchContextFragment {
    pub(crate) fn projected(
        projection: &ResearchContextProjection,
        replaces_previous: bool,
    ) -> Self {
        let mut sections = Vec::new();
        if replaces_previous {
            sections.push(
                "This bounded research context replaces the previously supplied research context."
                    .to_string(),
            );
        }
        sections.push(Self::projected_content(projection));
        Self {
            body: sections.join("\n\n"),
        }
    }

    pub(crate) fn projected_content(projection: &ResearchContextProjection) -> String {
        let mut body = "Treat these as project research evidence, not as user preference or absolute product truth. Re-evaluate them against the current task and current code."
            .to_string();
        for entry in &projection.entries {
            let subject = truncate_utf8(
                escape_context_text(entry.subject.as_str()).as_str(),
                MAX_RENDERED_SUBJECT_BYTES,
            );
            let statement = truncate_utf8(
                escape_context_text(entry.statement.as_str()).as_str(),
                MAX_RENDERED_STATEMENT_BYTES,
            );
            let line = format!(
                "- [{} · {}] {subject} — {statement}",
                status_name(entry.status),
                kind_name(entry.kind),
            );
            if body.len().saturating_add(line.len()).saturating_add(1) > MAX_PROJECTED_CONTENT_BYTES
            {
                continue;
            }
            body.push('\n');
            body.push_str(line.as_str());
        }
        body
    }

    pub(crate) fn removed() -> Self {
        Self {
            body: "The previously supplied bounded research context no longer applies to the current task."
                .to_string(),
        }
    }
}

impl ContextualUserFragment for ResearchContextFragment {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn body(&self) -> String {
        self.body.clone()
    }

    fn type_markers() -> (&'static str, &'static str) {
        (OPEN_MARKER, CLOSE_MARKER)
    }
}

fn status_name(status: ResearchStatus) -> &'static str {
    match status {
        ResearchStatus::Open => "open",
        ResearchStatus::Supported => "supported",
        ResearchStatus::Rejected => "rejected",
        ResearchStatus::Resolved => "resolved",
        ResearchStatus::Superseded => "superseded",
    }
}

fn kind_name(kind: ResearchEntryKind) -> &'static str {
    match kind {
        ResearchEntryKind::Fact => "fact",
        ResearchEntryKind::Constraint => "constraint",
        ResearchEntryKind::Goal => "goal",
        ResearchEntryKind::Unknown => "unknown",
        ResearchEntryKind::Hypothesis => "hypothesis",
        ResearchEntryKind::Exploration => "exploration",
        ResearchEntryKind::Decision => "decision",
        ResearchEntryKind::FuturePressure => "future_pressure",
        ResearchEntryKind::Outcome => "outcome",
    }
}

fn escape_context_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
#[path = "research_context_tests.rs"]
mod tests;
