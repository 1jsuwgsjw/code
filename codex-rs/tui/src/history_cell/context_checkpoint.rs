use super::HistoryCell;
use codex_app_server_protocol::ContextCheckpointDetails;
use ratatui::style::Stylize;
use ratatui::text::Line;

const EXPANDED_MIN_WIDTH: u16 = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TreeStyle {
    Unicode,
    Ascii,
}

#[derive(Debug)]
pub(crate) struct ContextCheckpointCell {
    checkpoint: Option<ContextCheckpointDetails>,
}

impl ContextCheckpointCell {
    pub(crate) fn new(checkpoint: Option<ContextCheckpointDetails>) -> Self {
        Self { checkpoint }
    }

    fn render_lines(&self, width: u16, tree_style: TreeStyle) -> Vec<Line<'static>> {
        let Some(checkpoint) = &self.checkpoint else {
            return vec!["* Context compacted".dim().into()];
        };

        let bullet = match tree_style {
            TreeStyle::Unicode => "• ",
            TreeStyle::Ascii => "* ",
        };
        let mut lines = vec![
            vec![
                bullet.cyan(),
                "Context checkpoint ".bold(),
                checkpoint.checkpoint_ref.clone().cyan().bold(),
            ]
            .into(),
        ];
        if width < EXPANDED_MIN_WIDTH {
            lines.push(
                format!(
                    "  {} state entries · window {}",
                    checkpoint.state_entry_count, checkpoint.window_number
                )
                .dim()
                .into(),
            );
            return lines;
        }

        let (branch, end) = match tree_style {
            TreeStyle::Unicode => ("  ├─ ", "  └─ "),
            TreeStyle::Ascii => ("  |- ", "  `- "),
        };
        let labels = if checkpoint.labels.is_empty() {
            "none".to_string()
        } else {
            checkpoint.labels.join(", ")
        };
        lines.push(
            vec![
                branch.dim(),
                "state ".into(),
                checkpoint.state_entry_count.to_string().bold(),
                format!(" · {labels}").dim(),
            ]
            .into(),
        );
        lines.push(
            vec![
                branch.dim(),
                format!(
                    "generation {} · record {}",
                    checkpoint.generation_id, checkpoint.turn_record_id
                )
                .into(),
            ]
            .into(),
        );
        let previous = checkpoint
            .previous_window_id
            .as_deref()
            .map(short_window_id)
            .unwrap_or("start");
        lines.push(
            vec![
                end.dim(),
                format!(
                    "window {} · {previous} -> {}",
                    checkpoint.window_number,
                    short_window_id(&checkpoint.window_id)
                )
                .into(),
            ]
            .into(),
        );
        lines
    }
}

impl HistoryCell for ContextCheckpointCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.render_lines(width, terminal_tree_style())
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let Some(checkpoint) = &self.checkpoint else {
            return vec!["Context compacted".into()];
        };
        vec![
            format!("Context checkpoint {}", checkpoint.checkpoint_ref).into(),
            format!(
                "  generation={} record={} stateEntries={} labels={}",
                checkpoint.generation_id,
                checkpoint.turn_record_id,
                checkpoint.state_entry_count,
                checkpoint.labels.join(",")
            )
            .into(),
            format!(
                "  window={} first={} previous={} current={}",
                checkpoint.window_number,
                checkpoint.first_window_id,
                checkpoint.previous_window_id.as_deref().unwrap_or("none"),
                checkpoint.window_id
            )
            .into(),
        ]
    }
}

fn short_window_id(window_id: &str) -> &str {
    window_id.get(..8).unwrap_or(window_id)
}

fn terminal_tree_style() -> TreeStyle {
    if cfg!(windows)
        && std::env::var_os("WT_SESSION").is_none()
        && std::env::var_os("TERM_PROGRAM").is_none()
        && std::env::var_os("ConEmuANSI").is_none()
        && std::env::var_os("ANSICON").is_none()
    {
        TreeStyle::Ascii
    } else {
        TreeStyle::Unicode
    }
}

#[cfg(test)]
#[path = "context_checkpoint_tests.rs"]
mod tests;
