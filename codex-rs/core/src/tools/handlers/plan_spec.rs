use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolSpec;
use serde_json::json;
use std::collections::BTreeMap;

pub fn create_update_plan_tool() -> ToolSpec {
    let plan_item_properties = BTreeMap::from([
        (
            "step".to_string(),
            JsonSchema::string(Some("Task step text.".to_string())),
        ),
        (
            "status".to_string(),
            JsonSchema::string_enum(
                vec![json!("pending"), json!("in_progress"), json!("completed")],
                Some("Step status.".to_string()),
            ),
        ),
    ]);

    let properties = BTreeMap::from([
        (
            "explanation".to_string(),
            JsonSchema::string(Some(
                "Optional explanation for this plan update.".to_string(),
            )),
        ),
        (
            "plan".to_string(),
            JsonSchema::array(
                JsonSchema::object(
                    plan_item_properties,
                    Some(vec!["step".to_string(), "status".to_string()]),
                    Some(false.into()),
                ),
                Some("The list of steps".to_string()),
            ),
        ),
        (
            "research_delta".to_string(),
            JsonSchema::array(
                research_delta_schema(),
                Some(
                    "Optional incremental research-state changes discovered while planning or executing the task. Submit only new or changed research entries; do not repeat the full research state."
                        .to_string(),
                ),
            ),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "update_plan".to_string(),
        description: r#"Updates the task plan.
Provide an optional explanation and a list of plan items, each with a step and status.
At most one step can be in_progress at a time.
Use research_delta only for durable task or project research such as confirmed facts, important unknowns, hypotheses, explorations, decisions, future pressures, and observed outcomes. User preferences are not product truth; record the underlying goal, evidence, or uncertainty instead.
"#
        .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["plan".to_string()]),
            Some(false.into()),
        ),
        output_schema: None,
    })
}

fn research_delta_schema() -> JsonSchema {
    let properties = BTreeMap::from([
        (
            "operation".to_string(),
            JsonSchema::string_enum(
                vec![json!("upsert"), json!("set_status"), json!("remove")],
                Some("Change to apply to the research entry.".to_string()),
            ),
        ),
        (
            "id".to_string(),
            JsonSchema::string(Some(
                "Stable identifier for this research entry within its scope.".to_string(),
            )),
        ),
        (
            "scope".to_string(),
            JsonSchema::string_enum(
                vec![json!("task"), json!("project")],
                Some(
                    "Task is limited to the current task; project is reusable only within the current project."
                        .to_string(),
                ),
            ),
        ),
        (
            "kind".to_string(),
            JsonSchema::string_enum(
                vec![
                    json!("fact"),
                    json!("constraint"),
                    json!("goal"),
                    json!("unknown"),
                    json!("hypothesis"),
                    json!("exploration"),
                    json!("decision"),
                    json!("future_pressure"),
                    json!("outcome"),
                ],
                Some("Research entry type. Required for upsert.".to_string()),
            ),
        ),
        (
            "subject".to_string(),
            JsonSchema::string(Some(
                "Short stable subject or dimension. Required for upsert.".to_string(),
            )),
        ),
        (
            "statement".to_string(),
            JsonSchema::string(Some(
                "Concise research statement. Required for upsert.".to_string(),
            )),
        ),
        (
            "status".to_string(),
            JsonSchema::string_enum(
                vec![
                    json!("open"),
                    json!("supported"),
                    json!("rejected"),
                    json!("resolved"),
                    json!("superseded"),
                ],
                Some(
                    "Entry status. Defaults to open for upsert and is required for set_status."
                        .to_string(),
                ),
            ),
        ),
    ]);

    JsonSchema::object(
        properties,
        Some(vec![
            "operation".to_string(),
            "id".to_string(),
            "scope".to_string(),
        ]),
        Some(false.into()),
    )
}

#[cfg(test)]
#[path = "plan_spec_tests.rs"]
mod tests;
