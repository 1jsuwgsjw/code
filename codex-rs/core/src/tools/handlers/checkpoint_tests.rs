use super::*;
use pretty_assertions::assert_eq;

#[test]
fn checkpoint_tool_specs_have_stable_names_and_required_fields() {
    let ToolSpec::Function(update) = update_summary_spec() else {
        panic!("update_summary must be a function tool");
    };
    let ToolSpec::Function(recall) = recall_checkpoint_artifact_spec() else {
        panic!("recall_checkpoint_artifact must be a function tool");
    };

    assert_eq!(update.name, "update_summary");
    assert_eq!(recall.name, "recall_checkpoint_artifact");
    let update_schema = serde_json::to_value(update.parameters).expect("serialize update schema");
    assert_eq!(
        update_schema["required"],
        serde_json::json!(["completedToolGroups", "summary", "nextAction"])
    );
    let recall_schema = serde_json::to_value(recall.parameters).expect("serialize recall schema");
    assert_eq!(
        recall_schema["required"],
        serde_json::json!(["artifactId"])
    );
}
