use super::*;
use pretty_assertions::assert_eq;

#[test]
fn checkpoint_tool_specs_have_stable_names_and_required_fields() {
    let ToolSpec::Function(update) = update_context_state_spec() else {
        panic!("update_context_state must be a function tool");
    };
    let ToolSpec::Function(recall) = recall_checkpoint_artifact_spec() else {
        panic!("recall_checkpoint_artifact must be a function tool");
    };

    assert_eq!(update.name, "update_context_state");
    assert_eq!(recall.name, "recall_checkpoint_artifact");
    let update_schema = serde_json::to_value(update.parameters).expect("serialize update schema");
    assert_eq!(
        update_schema["required"],
        serde_json::json!(["completedToolGroups", "toolGroupSettlements", "state"])
    );
    let recall_schema = serde_json::to_value(recall.parameters).expect("serialize recall schema");
    assert_eq!(recall_schema["required"], serde_json::Value::Null);
    assert!(recall_schema["properties"]["reference"].is_object());
}

#[test]
fn update_context_state_accepts_artifact_evidence_from_its_schema() {
    let request: UpdateContextStateRequest = serde_json::from_value(serde_json::json!({
        "completedToolGroups": [],
        "toolGroupSettlements": [],
        "state": {
            "active": {
                "objective": "preserve verified evidence",
                "entries": [{
                    "key": "verified.fact",
                    "kind": "sourceFact",
                    "content": "The retained fact has immutable evidence.",
                    "evidence": [{
                        "kind": "artifact",
                        "artifactId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }]
                }]
            }
        }
    }))
    .expect("tool-schema artifact evidence should deserialize");

    let expected = codex_context_checkpoint::EvidenceRef::Artifact {
        artifact_id: codex_context_checkpoint::ArtifactId::from_sha256("a".repeat(64)),
    };
    assert_eq!(
        request.state.active.entries[0].evidence,
        vec![expected.clone()]
    );
    assert_eq!(
        serde_json::to_value(&expected).expect("serialize artifact evidence"),
        serde_json::json!({
            "kind": "artifact",
            "artifactId": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        })
    );
    assert_eq!(
        serde_json::from_value::<codex_context_checkpoint::EvidenceRef>(serde_json::json!({
            "kind": "artifact",
            "artifact_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }))
        .expect("legacy persisted artifact evidence should remain readable"),
        expected
    );
}
