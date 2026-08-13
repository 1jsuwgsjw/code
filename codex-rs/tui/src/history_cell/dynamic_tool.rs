//! Dynamic tool-call history cells shared by live, replayed, and loaded transcripts.

use super::*;
use crate::text_formatting::format_json_compact;
use codex_app_server_protocol::DynamicToolCallOutputContentItem;

const MAX_ARGUMENT_BYTES: usize = 8 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_OUTPUT_ITEMS: usize = 32;
const MAX_VISIBLE_LINES: usize = 8;

#[derive(Debug, Clone)]
pub(crate) struct DynamicToolInvocation {
    pub(crate) namespace: Option<String>,
    pub(crate) tool: String,
    pub(crate) arguments: serde_json::Value,
}

#[derive(Debug)]
pub(crate) struct DynamicToolCallCell {
    call_id: String,
    invocation: DynamicToolInvocation,
    start_time: Instant,
    duration: Option<Duration>,
    content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
    success: Option<bool>,
    error: Option<String>,
    animations_enabled: bool,
}

impl DynamicToolCallCell {
    pub(crate) fn new(
        call_id: String,
        invocation: DynamicToolInvocation,
        animations_enabled: bool,
    ) -> Self {
        Self {
            call_id,
            invocation,
            start_time: Instant::now(),
            duration: None,
            content_items: None,
            success: None,
            error: None,
            animations_enabled,
        }
    }

    pub(crate) fn call_id(&self) -> &str {
        &self.call_id
    }

    pub(crate) fn complete(
        &mut self,
        duration: Duration,
        content_items: Option<Vec<DynamicToolCallOutputContentItem>>,
        success: Option<bool>,
        error: Option<String>,
    ) {
        self.duration = Some(duration);
        self.content_items = content_items;
        self.success = success;
        self.error = error;
    }

    pub(crate) fn mark_failed(&mut self) {
        self.duration = Some(self.start_time.elapsed());
        self.success = Some(false);
        self.error = Some("interrupted".to_string());
    }

    fn name(&self) -> String {
        self.invocation
            .namespace
            .as_ref()
            .map(|namespace| format!("{namespace}.{}", self.invocation.tool))
            .unwrap_or_else(|| self.invocation.tool.clone())
    }

    fn bounded_details(&self, width: usize) -> Vec<String> {
        let width = width.max(1);
        let arguments = serde_json::to_string(&self.invocation.arguments)
            .unwrap_or_else(|_| self.invocation.arguments.to_string());
        let arguments = format_json_compact(&arguments).unwrap_or(arguments);
        let mut details = vec![format!(
            "arguments: {}",
            bounded_text(&arguments, MAX_ARGUMENT_BYTES, MAX_VISIBLE_LINES, width)
        )];
        if let Some(items) = self.content_items.as_ref() {
            let mut output = Vec::new();
            for item in items.iter().take(MAX_OUTPUT_ITEMS) {
                match item {
                    DynamicToolCallOutputContentItem::InputText { text } => {
                        output.push(text.clone())
                    }
                    DynamicToolCallOutputContentItem::InputImage { image_url } => {
                        output.push(format!("[image] {image_url}"));
                    }
                }
            }
            if items.len() > MAX_OUTPUT_ITEMS {
                output.push("[truncated]".to_string());
            }
            if !output.is_empty() {
                details.push(format!(
                    "output: {}",
                    bounded_text(
                        &output.join("\n"),
                        MAX_OUTPUT_BYTES,
                        MAX_VISIBLE_LINES,
                        width
                    )
                ));
            }
        }
        if let Some(error) = self.error.as_deref() {
            details.push(format!(
                "error: {}",
                bounded_text(error, MAX_OUTPUT_BYTES, MAX_VISIBLE_LINES, width)
            ));
        }
        details
    }
}

impl HistoryCell for DynamicToolCallCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let bullet = match self.success {
            Some(true) => "•".green().bold(),
            Some(false) => "•".red().bold(),
            None => activity_indicator(
                Some(self.start_time),
                MotionMode::from_animations_enabled(self.animations_enabled),
                ReducedMotionIndicator::StaticBullet,
            )
            .unwrap_or_else(|| "•".dim()),
        };
        let verb = match self.success {
            Some(true) => "Completed",
            Some(false) => "Failed",
            None => "Calling",
        };
        let duration = self
            .duration
            .map(|duration| format!(" ({:.1}s)", duration.as_secs_f64()))
            .unwrap_or_default();
        let mut lines = vec![
            vec![
                bullet,
                " ".into(),
                verb.bold(),
                " ".into(),
                self.name().cyan(),
                duration.dim(),
            ]
            .into(),
        ];
        let detail_width = usize::from(width).saturating_sub(4).max(1);
        let details = self
            .bounded_details(detail_width)
            .into_iter()
            .flat_map(|detail| {
                textwrap::wrap(&detail, detail_width)
                    .into_iter()
                    .map(std::borrow::Cow::into_owned)
                    .collect::<Vec<_>>()
            })
            .map(|detail| Line::from(detail.dim()))
            .collect::<Vec<_>>();
        lines.extend(prefix_lines(details, "  └ ".dim(), "    ".into()));
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let state = match self.success {
            Some(true) => "completed",
            Some(false) => "failed",
            None => "in progress",
        };
        let mut lines = vec![Line::from(format!("Tool {} · {state}", self.name()))];
        lines.extend(
            self.bounded_details(RAW_TOOL_OUTPUT_WIDTH)
                .into_iter()
                .flat_map(|detail| raw_lines_from_source(&detail)),
        );
        lines
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        if !self.animations_enabled || self.success.is_some() {
            return None;
        }
        Some((self.start_time.elapsed().as_millis() / 50) as u64)
    }
}

pub(crate) fn new_active_dynamic_tool_call(
    call_id: String,
    invocation: DynamicToolInvocation,
    animations_enabled: bool,
) -> DynamicToolCallCell {
    DynamicToolCallCell::new(call_id, invocation, animations_enabled)
}

pub(crate) fn dynamic_tool_call_cell_from_item(
    item: &codex_app_server_protocol::ThreadItem,
) -> Option<DynamicToolCallCell> {
    let codex_app_server_protocol::ThreadItem::DynamicToolCall {
        id,
        namespace,
        tool,
        arguments,
        status,
        content_items,
        success,
        error,
        duration_ms,
    } = item
    else {
        return None;
    };
    let mut cell = DynamicToolCallCell::new(
        id.clone(),
        DynamicToolInvocation {
            namespace: namespace.clone(),
            tool: tool.clone(),
            arguments: arguments.clone(),
        },
        /*animations_enabled*/ false,
    );
    if !matches!(
        status,
        codex_app_server_protocol::DynamicToolCallStatus::InProgress
    ) {
        cell.complete(
            Duration::from_millis((*duration_ms).unwrap_or_default().max(0) as u64),
            content_items.clone(),
            (*success).or(match status {
                codex_app_server_protocol::DynamicToolCallStatus::InProgress => None,
                codex_app_server_protocol::DynamicToolCallStatus::Completed => Some(true),
                codex_app_server_protocol::DynamicToolCallStatus::Failed => Some(false),
            }),
            error.clone(),
        );
    }
    Some(cell)
}

fn bounded_text(text: &str, max_bytes: usize, max_lines: usize, width: usize) -> String {
    let (text, byte_truncated) = if text.len() > max_bytes {
        let end = floor_char_boundary(text, max_bytes.saturating_sub(" [truncated]".len()));
        (&text[..end], true)
    } else {
        (text, false)
    };
    let mut wrapped = textwrap::wrap(text, width.max(1))
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let line_truncated = wrapped.len() > max_lines;
    wrapped.truncate(max_lines);
    let mut output = wrapped.join("\n");
    if byte_truncated || line_truncated {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str("[truncated]");
    }
    output
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
