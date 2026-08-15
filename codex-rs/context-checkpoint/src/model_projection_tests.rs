use super::*;
use crate::ActiveContextState;
use crate::ContextStateSnapshot;
use crate::ToolGroupId;
use pretty_assertions::assert_eq;
use serde_json::json;

fn memory_entry(key: &str, content: &str) -> StateEntry {
    StateEntry {
        key: key.to_string(),
        kind: StateEntryKind::UserDecision,
        content: content.to_string(),
        evidence: Vec::new(),
        source_revision: None,
    }
}

#[test]
fn projection_uses_semantic_references_and_hides_content_hashes() {
    let artifact_id = ArtifactId::from_sha256("a".repeat(64));
    let record = TurnRecord {
        record_id: TurnRecordId::new(1),
        generation_id: CheckpointGenerationId::new(1),
        completed_groups: vec![ToolGroupId::new(1)],
        tool_group_settlements: Vec::new(),
        state: ContextStateSnapshot {
            quarter: Vec::new(),
            session: Vec::new(),
            active: ActiveContextState {
                objective: "Preserve the verified source fact.".to_string(),
                entries: vec![StateEntry {
                    key: "source.smoke".to_string(),
                    kind: StateEntryKind::SourceFact,
                    content: "source.txt contains revision v1".to_string(),
                    evidence: vec![
                        EvidenceRef::WorkspacePath {
                            path: "source.txt".to_string(),
                        },
                        EvidenceRef::Artifact {
                            artifact_id: artifact_id.clone(),
                        },
                    ],
                    source_revision: Some(format!("sha256:{}", "b".repeat(64))),
                }],
                constraints: Vec::new(),
                open_questions: Vec::new(),
                continuity_hints: vec![
                    "The verified source fact may be reused without rereading source.txt."
                        .to_string(),
                ],
            },
        },
        state_removals: Vec::new(),
        summary: String::new(),
        evidence: vec![EvidenceRef::Artifact {
            artifact_id: artifact_id.clone(),
        }],
        changes: Vec::new(),
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        correction_of: None,
    };

    let projected: serde_json::Value =
        serde_json::from_str(&model_checkpoint_json(&record).expect("projection should serialize"))
            .expect("projection should be valid JSON");

    assert_eq!(
        projected,
        json!({
            "checkpointRef": "ctx:G000001/TR000001",
            "active": {
                "objective": "Preserve the verified source fact.",
                "entries": [{
                    "reference": "fact:source/source.smoke",
                    "content": "source.txt contains revision v1",
                    "evidenceRefs": ["workspace:source.txt", "artifact:TR000001/A001"],
                    "sourceStatus": "verified"
                }],
                "continuityHints": [
                    "The verified source fact may be reused without rereading source.txt."
                ]
            },
            "recallRefs": ["artifact:TR000001/A001"]
        })
    );
    assert_eq!(
        model_checkpoint_view(&record).expect("model view should render"),
        concat!(
            "Checkpoint ctx:G000001/TR000001\n",
            "\n",
            "Active\n",
            "  Objective\n",
            "    Preserve the verified source fact.\n",
            "  fact:source/source.smoke\n",
            "    source.txt contains revision v1\n",
            "    evidence: workspace:source.txt · artifact:TR000001/A001\n",
            "    source: verified\n",
            "  May matter later\n",
            "    - The verified source fact may be reused without rereading source.txt."
        )
    );
    assert_eq!(
        artifact_id_for_reference(&record, "artifact:TR000001/A001"),
        Some(&artifact_id)
    );
}

#[test]
fn legacy_summary_projection_preserves_semantics_without_hashes() {
    let record = TurnRecord {
        record_id: TurnRecordId::new(2),
        generation_id: CheckpointGenerationId::new(2),
        completed_groups: Vec::new(),
        tool_group_settlements: Vec::new(),
        state: ContextStateSnapshot {
            quarter: Vec::new(),
            session: Vec::new(),
            active: ActiveContextState {
                objective: String::new(),
                entries: Vec::new(),
                constraints: Vec::new(),
                open_questions: Vec::new(),
                continuity_hints: vec!["Parser knowledge may be relevant later.".to_string()],
            },
        },
        state_removals: Vec::new(),
        summary: "The parser accepts semantic references.".to_string(),
        evidence: vec![EvidenceRef::Artifact {
            artifact_id: ArtifactId::from_sha256("c".repeat(64)),
        }],
        changes: Vec::new(),
        validation: vec!["workflow passed".to_string()],
        decisions: Vec::new(),
        open_items: Vec::new(),
        correction_of: None,
    };

    let projected = model_checkpoint_json(&record).expect("projection should serialize");

    assert!(projected.contains("The parser accepts semantic references."));
    assert!(projected.contains("Parser knowledge may be relevant later."));
    assert!(projected.contains("artifact:TR000002/A001"));
    assert!(!projected.contains(&"c".repeat(64)));
}

#[test]
fn model_view_materializes_all_memory_horizons_without_replaying_history() {
    let record = TurnRecord {
        record_id: TurnRecordId::new(3),
        generation_id: CheckpointGenerationId::new(4),
        completed_groups: Vec::new(),
        tool_group_settlements: Vec::new(),
        state: ContextStateSnapshot {
            quarter: vec![memory_entry(
                "quarter.architecture",
                "Compression keeps effective knowledge and archives process noise.",
            )],
            session: vec![memory_entry(
                "session.runtime",
                "The checkpoint runtime validates every settled tool group.",
            )],
            active: ActiveContextState {
                objective: "Render a sparse model checkpoint view.".to_string(),
                entries: vec![memory_entry(
                    "active.focus",
                    "The current change replaces JSON prompt presentation.",
                )],
                ..Default::default()
            },
        },
        state_removals: Vec::new(),
        summary: String::new(),
        evidence: Vec::new(),
        changes: Vec::new(),
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        correction_of: None,
    };

    assert_eq!(
        model_checkpoint_view(&record).expect("model view should render"),
        concat!(
            "Checkpoint ctx:G000004/TR000003\n",
            "\n",
            "Quarter\n",
            "  decision/quarter.architecture\n",
            "    Compression keeps effective knowledge and archives process noise.\n",
            "\n",
            "Session\n",
            "  decision/session.runtime\n",
            "    The checkpoint runtime validates every settled tool group.\n",
            "\n",
            "Active\n",
            "  Objective\n",
            "    Render a sparse model checkpoint view.\n",
            "  decision/active.focus\n",
            "    The current change replaces JSON prompt presentation."
        )
    );
}
