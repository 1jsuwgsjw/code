use super::*;
use crate::ArtifactId;
use pretty_assertions::assert_eq;
use serde_json::json;

fn artifact() -> ArtifactRef {
    ArtifactRef {
        artifact_id: ArtifactId::from_sha256("a".repeat(64)),
        sha256: "a".repeat(64),
        byte_len: 0,
        media_type: "text/plain".to_string(),
    }
}

#[test]
fn outline_returns_line_windows_without_artifact_content() {
    let data = (1..=61)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();

    let result = build_recall_result(artifact(), data, ArtifactRecallSelection::Outline, 4 * 1024)
        .expect("build outline");

    assert_eq!(
        result.view,
        ArtifactRecallView::Outline {
            line_count: 61,
            sections: vec![
                ArtifactOutlineSection {
                    start_line: 1,
                    end_line: 20,
                    approximate_bytes: 151,
                    preview: "line 1".to_string(),
                },
                ArtifactOutlineSection {
                    start_line: 21,
                    end_line: 40,
                    approximate_bytes: 160,
                    preview: "line 21".to_string(),
                },
                ArtifactOutlineSection {
                    start_line: 41,
                    end_line: 60,
                    approximate_bytes: 160,
                    preview: "line 41".to_string(),
                },
                ArtifactOutlineSection {
                    start_line: 61,
                    end_line: 61,
                    approximate_bytes: 8,
                    preview: "line 61".to_string(),
                },
            ],
            truncated: false,
            next_start_line: None,
        }
    );
}

#[test]
fn lines_returns_only_the_requested_numbered_range() {
    let result = build_recall_result(
        artifact(),
        b"zero\none\ntwo\nthree".to_vec(),
        ArtifactRecallSelection::Lines {
            start_line: 2,
            end_line: 3,
        },
        1024,
    )
    .expect("recall selected lines");

    assert_eq!(
        result.view,
        ArtifactRecallView::Lines {
            line_count: 4,
            start_line: 2,
            end_line: 3,
            content: "L2: one\nL3: two\n".to_string(),
            truncated: false,
            next_start_line: None,
        }
    );
}

#[test]
fn search_returns_bounded_matches_with_context() {
    let result = build_recall_result(
        artifact(),
        b"alpha\nbefore\nNeedle value\nafter\nneedle again".to_vec(),
        ArtifactRecallSelection::Search {
            query: "needle".to_string(),
            context_lines: 1,
            max_matches: 1,
        },
        1024,
    )
    .expect("search artifact");

    assert_eq!(
        result.view,
        ArtifactRecallView::Search {
            line_count: 5,
            query: "needle".to_string(),
            matches: vec![ArtifactSearchMatch {
                line: 3,
                column: 1,
                excerpt: "Needle value".to_string(),
                before: vec!["L2: before".to_string()],
                after: vec!["L4: after".to_string()],
            }],
            total_matches: 2,
            truncated: true,
        }
    );
}

#[test]
fn legacy_mcp_json_is_recalled_as_searchable_text_lines() {
    let mut legacy_artifact = artifact();
    legacy_artifact.media_type = "application/json".to_string();
    let data = serde_json::to_vec(&json!({
        "content": [
            {"type": "text", "text": "zero\none"},
            {"type": "text", "text": "two\nthree"}
        ],
        "isError": false
    }))
    .expect("serialize legacy MCP result");

    let result = build_recall_result(
        legacy_artifact,
        data,
        ArtifactRecallSelection::Lines {
            start_line: 2,
            end_line: 3,
        },
        1_024,
    )
    .expect("recall legacy MCP text");

    assert_eq!(
        result.view,
        ArtifactRecallView::Lines {
            line_count: 4,
            start_line: 2,
            end_line: 3,
            content: "L2: one\nL3: two\n".to_string(),
            truncated: false,
            next_start_line: None,
        }
    );
}

#[test]
fn legacy_mcp_json_keeps_mixed_content_structured() {
    let mut legacy_artifact = artifact();
    legacy_artifact.media_type = "application/json".to_string();
    let data = serde_json::to_vec(&json!({
        "content": [
            {"type": "text", "text": "caption"},
            {"type": "image", "data": "aW1hZ2U=", "mimeType": "image/png"}
        ]
    }))
    .expect("serialize mixed MCP result");

    assert_eq!(legacy_mcp_text_archive(&legacy_artifact, &data), None);
}
