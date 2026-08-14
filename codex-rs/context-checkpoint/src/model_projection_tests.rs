use super::*;
use crate::ActiveContextState;
use crate::ContextStateSnapshot;
use crate::ToolGroupId;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn projection_uses_semantic_references_and_hides_content_hashes() {
    let artifact_id = ArtifactId::from_sha256("a".repeat(64));
    let record = TurnRecord {
        record_id: TurnRecordId::new(1),
        generation_id: CheckpointGenerationId::new(1),
        completed_groups: vec![ToolGroupId::new(1)],
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
                next_action: "Resume without rereading source.txt.".to_string(),
            },
        },
        summary: String::new(),
        evidence: vec![EvidenceRef::Artifact {
            artifact_id: artifact_id.clone(),
        }],
        changes: Vec::new(),
        validation: Vec::new(),
        decisions: Vec::new(),
        open_items: Vec::new(),
        next_action: String::new(),
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
                "nextAction": "Resume without rereading source.txt."
            },
            "recallRefs": ["artifact:TR000001/A001"]
        })
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
        state: ContextStateSnapshot {
            quarter: Vec::new(),
            session: Vec::new(),
            active: ActiveContextState {
                objective: String::new(),
                entries: Vec::new(),
                constraints: Vec::new(),
                open_questions: Vec::new(),
                next_action: String::new(),
            },
        },
        summary: "The parser accepts semantic references.".to_string(),
        evidence: vec![EvidenceRef::Artifact {
            artifact_id: ArtifactId::from_sha256("c".repeat(64)),
        }],
        changes: Vec::new(),
        validation: vec!["workflow passed".to_string()],
        decisions: Vec::new(),
        open_items: Vec::new(),
        next_action: "resume parser work".to_string(),
        correction_of: None,
    };

    let projected = model_checkpoint_json(&record).expect("projection should serialize");

    assert!(projected.contains("The parser accepts semantic references."));
    assert!(projected.contains("resume parser work"));
    assert!(projected.contains("artifact:TR000002/A001"));
    assert!(!projected.contains(&"c".repeat(64)));
}
