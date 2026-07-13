use super::create_update_plan_tool;
use codex_tools::ToolSpec;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn update_plan_exposes_stable_research_delta_schema() {
    let ToolSpec::Function(tool) = create_update_plan_tool() else {
        panic!("update_plan should be a function tool");
    };
    let schema = serde_json::to_value(tool.parameters).expect("schema should serialize");
    let research_delta = &schema["properties"]["research_delta"];

    assert_eq!(research_delta["type"], json!("array"));
    assert_eq!(
        research_delta["items"]["required"],
        json!(["operation", "id", "scope"])
    );
    assert_eq!(
        research_delta["items"]["properties"]["scope"]["enum"],
        json!(["task", "project"])
    );
    assert_eq!(
        research_delta["items"]["properties"]["operation"]["enum"],
        json!(["upsert", "set_status", "remove"])
    );
}
