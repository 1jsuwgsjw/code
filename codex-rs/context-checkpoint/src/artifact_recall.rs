use crate::ArtifactLocator;
use crate::ArtifactRef;
use crate::CheckpointError;
use serde::Serialize;

const MAX_LINE_SELECTION: usize = 200;
const MAX_OUTLINE_SECTIONS: usize = 24;
const MIN_OUTLINE_SECTION_LINES: usize = 20;
const MAX_SEARCH_CONTEXT_LINES: usize = 10;
const MAX_SEARCH_MATCHES: usize = 20;
const MAX_SEARCH_QUERY_CHARS: usize = 256;
const OUTLINE_PREVIEW_CHARS: usize = 120;
const SEARCH_EXCERPT_CONTEXT_CHARS: usize = 80;
const SEARCH_CONTEXT_LINE_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactRecallSelection {
    Outline,
    Lines {
        start_line: usize,
        end_line: usize,
    },
    Search {
        query: String,
        context_lines: usize,
        max_matches: usize,
    },
    Prefix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactOutlineSection {
    pub start_line: usize,
    pub end_line: usize,
    pub approximate_bytes: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSearchMatch {
    pub line: usize,
    pub column: usize,
    pub excerpt: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactRecallView {
    Outline {
        line_count: usize,
        sections: Vec<ArtifactOutlineSection>,
        truncated: bool,
        next_start_line: Option<usize>,
    },
    Lines {
        line_count: usize,
        start_line: usize,
        end_line: usize,
        content: String,
        truncated: bool,
        next_start_line: Option<usize>,
    },
    Search {
        line_count: usize,
        query: String,
        matches: Vec<ArtifactSearchMatch>,
        total_matches: usize,
        truncated: bool,
    },
    Prefix {
        line_count: usize,
        content: String,
        truncated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallRequest {
    pub locator: ArtifactLocator,
    pub selection: ArtifactRecallSelection,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallResult {
    pub artifact: ArtifactRef,
    pub view: ArtifactRecallView,
}

pub(crate) fn build_recall_result(
    artifact: ArtifactRef,
    data: Vec<u8>,
    selection: ArtifactRecallSelection,
    max_bytes: usize,
) -> Result<RecallResult, CheckpointError> {
    let text = String::from_utf8_lossy(&data);
    let lines = text.lines().collect::<Vec<_>>();
    let view = match selection {
        ArtifactRecallSelection::Outline => outline_view(&lines, max_bytes),
        ArtifactRecallSelection::Lines {
            start_line,
            end_line,
        } => lines_view(&lines, start_line, end_line, max_bytes)?,
        ArtifactRecallSelection::Search {
            query,
            context_lines,
            max_matches,
        } => search_view(&lines, query, context_lines, max_matches, max_bytes)?,
        ArtifactRecallSelection::Prefix => ArtifactRecallView::Prefix {
            line_count: lines.len(),
            content: truncate_utf8(&text, max_bytes),
            truncated: text.len() > max_bytes,
        },
    };
    Ok(RecallResult { artifact, view })
}

fn outline_view(lines: &[&str], max_bytes: usize) -> ArtifactRecallView {
    if lines.is_empty() {
        return ArtifactRecallView::Outline {
            line_count: 0,
            sections: Vec::new(),
            truncated: false,
            next_start_line: None,
        };
    }

    let section_lines = lines
        .len()
        .div_ceil(MAX_OUTLINE_SECTIONS)
        .max(MIN_OUTLINE_SECTION_LINES);
    let mut sections = Vec::new();
    let mut estimated_output_bytes = 0;
    for start_index in (0..lines.len()).step_by(section_lines) {
        let end_index = (start_index + section_lines).min(lines.len());
        let preview = lines[start_index..end_index]
            .iter()
            .find(|line| !line.trim().is_empty())
            .map(|line| truncate_chars(line.trim(), OUTLINE_PREVIEW_CHARS))
            .unwrap_or_default();
        let approximate_bytes = lines[start_index..end_index]
            .iter()
            .map(|line| line.len() + 1)
            .sum();
        let section_output_bytes = preview.len() + 96;
        if !sections.is_empty() && estimated_output_bytes + section_output_bytes > max_bytes {
            break;
        }
        estimated_output_bytes += section_output_bytes;
        sections.push(ArtifactOutlineSection {
            start_line: start_index + 1,
            end_line: end_index,
            approximate_bytes,
            preview,
        });
    }
    let next_start_line = sections
        .last()
        .and_then(|section| (section.end_line < lines.len()).then_some(section.end_line + 1));
    ArtifactRecallView::Outline {
        line_count: lines.len(),
        truncated: next_start_line.is_some(),
        next_start_line,
        sections,
    }
}

fn lines_view(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    max_bytes: usize,
) -> Result<ArtifactRecallView, CheckpointError> {
    if start_line == 0 || end_line < start_line {
        return Err(CheckpointError::InvalidRequest(
            "lines mode requires 1-based startLine <= endLine".to_string(),
        ));
    }
    let selected_line_count = end_line
        .checked_sub(start_line)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CheckpointError::InvalidRequest("line range is too large".to_string()))?;
    if selected_line_count > MAX_LINE_SELECTION {
        return Err(CheckpointError::InvalidRequest(format!(
            "lines mode is limited to {MAX_LINE_SELECTION} lines per recall"
        )));
    }
    if start_line > lines.len() {
        return Err(CheckpointError::InvalidRequest(format!(
            "startLine {start_line} exceeds artifact line count {}",
            lines.len()
        )));
    }

    let requested_end_line = end_line.min(lines.len());
    let (content, rendered_end_line, next_start_line) =
        render_numbered_lines(lines, start_line, requested_end_line, max_bytes);
    Ok(ArtifactRecallView::Lines {
        line_count: lines.len(),
        start_line,
        end_line: rendered_end_line,
        content,
        truncated: next_start_line.is_some(),
        next_start_line,
    })
}

fn search_view(
    lines: &[&str],
    query: String,
    context_lines: usize,
    max_matches: usize,
    max_bytes: usize,
) -> Result<ArtifactRecallView, CheckpointError> {
    if query.trim().is_empty() {
        return Err(CheckpointError::InvalidRequest(
            "search mode requires a non-empty query".to_string(),
        ));
    }
    if query.chars().count() > MAX_SEARCH_QUERY_CHARS {
        return Err(CheckpointError::InvalidRequest(format!(
            "search query is limited to {MAX_SEARCH_QUERY_CHARS} characters"
        )));
    }
    if context_lines > MAX_SEARCH_CONTEXT_LINES {
        return Err(CheckpointError::InvalidRequest(format!(
            "contextLines is limited to {MAX_SEARCH_CONTEXT_LINES}"
        )));
    }
    if max_matches == 0 || max_matches > MAX_SEARCH_MATCHES {
        return Err(CheckpointError::InvalidRequest(format!(
            "maxMatches must be between 1 and {MAX_SEARCH_MATCHES}"
        )));
    }

    let needle = query.to_ascii_lowercase();
    let mut total_matches = 0;
    let mut positions = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let haystack = line.to_ascii_lowercase();
        let mut search_start = 0;
        while let Some(relative_index) = haystack[search_start..].find(needle.as_str()) {
            let byte_index = search_start + relative_index;
            total_matches += 1;
            if positions.len() < max_matches {
                positions.push((line_index, byte_index));
            }
            search_start = byte_index + needle.len();
        }
    }

    let mut matches = Vec::new();
    let mut estimated_output_bytes = 0;
    for (line_index, byte_index) in positions {
        let line = lines[line_index];
        let excerpt = search_excerpt(line, byte_index, query.len());
        let before = context_before(lines, line_index, context_lines);
        let after = context_after(lines, line_index, context_lines);
        let match_output_bytes = excerpt.len()
            + before.iter().map(String::len).sum::<usize>()
            + after.iter().map(String::len).sum::<usize>()
            + 96;
        if estimated_output_bytes + match_output_bytes > max_bytes {
            break;
        }
        estimated_output_bytes += match_output_bytes;
        matches.push(ArtifactSearchMatch {
            line: line_index + 1,
            column: line[..byte_index].chars().count() + 1,
            excerpt,
            before,
            after,
        });
    }
    let truncated = matches.len() < total_matches;
    Ok(ArtifactRecallView::Search {
        line_count: lines.len(),
        query,
        matches,
        total_matches,
        truncated,
    })
}

fn render_numbered_lines(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    max_bytes: usize,
) -> (String, usize, Option<usize>) {
    let mut content = String::new();
    let mut rendered_end_line = start_line.saturating_sub(1);
    for line_number in start_line..=end_line {
        let rendered = format!("L{line_number}: {}\n", lines[line_number - 1]);
        if content.len() + rendered.len() <= max_bytes {
            content.push_str(rendered.as_str());
            rendered_end_line = line_number;
            continue;
        }
        if content.is_empty() {
            content = truncate_utf8(rendered.as_str(), max_bytes);
            rendered_end_line = line_number;
        }
        return (content, rendered_end_line, Some(line_number));
    }
    (content, rendered_end_line, None)
}

fn context_before(lines: &[&str], line_index: usize, count: usize) -> Vec<String> {
    let start = line_index.saturating_sub(count);
    (start..line_index)
        .map(|index| {
            format!(
                "L{}: {}",
                index + 1,
                truncate_chars(lines[index], SEARCH_CONTEXT_LINE_CHARS)
            )
        })
        .collect()
}

fn context_after(lines: &[&str], line_index: usize, count: usize) -> Vec<String> {
    let end = (line_index + count + 1).min(lines.len());
    (line_index + 1..end)
        .map(|index| {
            format!(
                "L{}: {}",
                index + 1,
                truncate_chars(lines[index], SEARCH_CONTEXT_LINE_CHARS)
            )
        })
        .collect()
}

fn search_excerpt(line: &str, byte_index: usize, query_bytes: usize) -> String {
    let match_start = line[..byte_index].chars().count();
    let match_chars = line[byte_index..byte_index + query_bytes].chars().count();
    let chars = line.chars().collect::<Vec<_>>();
    let excerpt_start = match_start.saturating_sub(SEARCH_EXCERPT_CONTEXT_CHARS);
    let excerpt_end = (match_start + match_chars + SEARCH_EXCERPT_CONTEXT_CHARS).min(chars.len());
    let mut excerpt = chars[excerpt_start..excerpt_end].iter().collect::<String>();
    if excerpt_start > 0 {
        excerpt.insert_str(0, "...");
    }
    if excerpt_end < chars.len() {
        excerpt.push_str("...");
    }
    excerpt
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        truncated.push_str("...");
    }
    truncated
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

#[cfg(test)]
#[path = "artifact_recall_tests.rs"]
mod tests;
