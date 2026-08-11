//! Read-only research-state history cells.

use super::*;
use codex_app_server_protocol::TurnResearchEntryKind;
use codex_app_server_protocol::TurnResearchScope;
use codex_app_server_protocol::TurnResearchStateEntry;
use codex_app_server_protocol::TurnResearchStateUpdatedNotification;
use codex_app_server_protocol::TurnResearchStatus;

pub(crate) fn new_research_state_update(
    update: TurnResearchStateUpdatedNotification,
) -> ResearchStateUpdateCell {
    ResearchStateUpdateCell {
        revision: update.revision,
        changed: update.changed,
        entries: update.entries,
    }
}

#[derive(Debug)]
pub(crate) struct ResearchStateUpdateCell {
    revision: u64,
    changed: bool,
    entries: Vec<TurnResearchStateEntry>,
}

impl HistoryCell for ResearchStateUpdateCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut header = vec!["• ".dim(), "Research State".bold()];
        header.push(format!(" · rev {}", self.revision).dim());
        if !self.changed {
            header.push(" · unchanged".dim().italic());
        }

        let mut lines = vec![Line::from(header)];
        let mut body = Vec::new();
        if self.entries.is_empty() {
            body.push(Line::from("(empty)".dim().italic()));
        } else {
            for entry in &self.entries {
                body.extend(render_entry(entry, width));
            }
        }
        lines.extend(prefix_lines(body, "  └ ".dim(), "    ".into()));
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let change = if self.changed { "changed" } else { "unchanged" };
        let mut lines = vec![Line::from(format!(
            "Research State revision {} ({change})",
            self.revision
        ))];
        if self.entries.is_empty() {
            lines.push(Line::from("(empty)"));
        } else {
            lines.extend(self.entries.iter().map(|entry| {
                Line::from(format!(
                    "{} {} [{}] {}: {}",
                    status_label(entry.status),
                    kind_label(entry.kind),
                    scope_label(entry.scope),
                    entry.subject,
                    entry.statement
                ))
            }));
        }
        lines
    }
}

fn render_entry(entry: &TurnResearchStateEntry, width: u16) -> Vec<Line<'static>> {
    let (symbol, status_style) = status_display(entry.status);
    let line = Line::from(vec![
        format!("{symbol} ").set_style(status_style),
        kind_label(entry.kind).bold(),
        format!(
            " · {} [{} · {}] — ",
            entry.subject,
            scope_label(entry.scope),
            status_label(entry.status)
        )
        .dim(),
        entry.statement.clone().into(),
    ]);
    let wrapped = adaptive_wrap_line(
        &line,
        RtOptions::new(width.saturating_sub(4).max(1) as usize).subsequent_indent("  ".into()),
    );
    let mut out = Vec::new();
    push_owned_lines(&wrapped, &mut out);
    out
}

fn status_display(status: TurnResearchStatus) -> (&'static str, Style) {
    match status {
        TurnResearchStatus::Open => ("?", Style::default().cyan()),
        TurnResearchStatus::Supported => ("✓", Style::default().green().bold()),
        TurnResearchStatus::Rejected => ("×", Style::default().red()),
        TurnResearchStatus::Resolved => ("◆", Style::default().cyan()),
        TurnResearchStatus::Superseded => ("↺", Style::default().dim()),
    }
}

fn status_label(status: TurnResearchStatus) -> &'static str {
    match status {
        TurnResearchStatus::Open => "open",
        TurnResearchStatus::Supported => "supported",
        TurnResearchStatus::Rejected => "rejected",
        TurnResearchStatus::Resolved => "resolved",
        TurnResearchStatus::Superseded => "superseded",
    }
}

fn scope_label(scope: TurnResearchScope) -> &'static str {
    match scope {
        TurnResearchScope::Task => "task",
        TurnResearchScope::Project => "project",
    }
}

fn kind_label(kind: TurnResearchEntryKind) -> &'static str {
    match kind {
        TurnResearchEntryKind::Fact => "Fact",
        TurnResearchEntryKind::Constraint => "Constraint",
        TurnResearchEntryKind::Goal => "Goal",
        TurnResearchEntryKind::Unknown => "Unknown",
        TurnResearchEntryKind::Hypothesis => "Hypothesis",
        TurnResearchEntryKind::Exploration => "Exploration",
        TurnResearchEntryKind::Decision => "Decision",
        TurnResearchEntryKind::FuturePressure => "Future Pressure",
        TurnResearchEntryKind::Outcome => "Outcome",
    }
}
