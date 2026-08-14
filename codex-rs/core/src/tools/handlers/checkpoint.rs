use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_context_checkpoint::ArtifactId;
use codex_context_checkpoint::RecallRequest;
use codex_context_checkpoint::UpdateContextStateRequest;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

pub struct UpdateContextStateHandler;

impl ToolExecutor<ToolInvocation> for UpdateContextStateHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("update_context_state")
    }

    fn spec(&self) -> ToolSpec {
        update_context_state_spec()
    }

    #[allow(deprecated)]
    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "update_context_state received an unsupported payload".to_string(),
                ));
            };
            let request =
                serde_json::from_str::<UpdateContextStateRequest>(&arguments).map_err(|error| {
                    FunctionCallError::RespondToModel(format!(
                        "failed to parse update_context_state arguments: {error}"
                    ))
                })?;
            let pending = invocation
                .session
                .services
                .context_checkpoint
                .prepare_context_state(request, invocation.turn.cwd.as_path())
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let output = json!({
                "status": "pending",
                "recordId": pending.record.record_id.to_string(),
                "generationId": pending.record.generation_id.to_string(),
                "historyStart": pending.history_start,
                "historyEnd": pending.history_end,
                "manifestSha256": pending.manifest_sha256,
            });
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                output.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for UpdateContextStateHandler {}

pub struct RecallCheckpointArtifactHandler;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecallArguments {
    artifact_id: String,
    #[serde(default = "default_recall_bytes")]
    max_bytes: usize,
}

impl ToolExecutor<ToolInvocation> for RecallCheckpointArtifactHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("recall_checkpoint_artifact")
    }

    fn spec(&self) -> ToolSpec {
        recall_checkpoint_artifact_spec()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "recall_checkpoint_artifact received an unsupported payload".to_string(),
                ));
            };
            let arguments =
                serde_json::from_str::<RecallArguments>(&arguments).map_err(|error| {
                    FunctionCallError::RespondToModel(format!(
                        "failed to parse recall_checkpoint_artifact arguments: {error}"
                    ))
                })?;
            let recalled = invocation
                .session
                .services
                .context_checkpoint
                .recall(RecallRequest {
                    artifact_id: ArtifactId::from_sha256(arguments.artifact_id),
                    max_bytes: arguments.max_bytes,
                })
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let output = json!({
                "artifactId": recalled.artifact.artifact_id.as_str(),
                "sha256": recalled.artifact.sha256,
                "mediaType": recalled.artifact.media_type,
                "byteLength": recalled.artifact.byte_len,
                "truncated": recalled.truncated,
                "content": String::from_utf8_lossy(&recalled.data),
            });
            Ok(boxed_tool_output(FunctionToolOutput::from_text(
                output.to_string(),
                Some(true),
            )))
        })
    }
}

impl CoreToolRuntime for RecallCheckpointArtifactHandler {}

fn update_context_state_spec() -> ToolSpec {
    let string_array = |description: &str| {
        JsonSchema::array(JsonSchema::string(None), Some(description.to_string()))
    };
    let evidence = JsonSchema::object(
        BTreeMap::from([
            (
                "kind".to_string(),
                JsonSchema::string_enum(
                    vec![
                        serde_json::Value::String("artifact".to_string()),
                        serde_json::Value::String("workspacePath".to_string()),
                        serde_json::Value::String("symbol".to_string()),
                    ],
                    Some("Evidence reference kind.".to_string()),
                ),
            ),
            (
                "artifactId".to_string(),
                JsonSchema::string(Some("Recorded artifact SHA-256 id.".to_string())),
            ),
            (
                "path".to_string(),
                JsonSchema::string(Some("Workspace path evidence.".to_string())),
            ),
            (
                "value".to_string(),
                JsonSchema::string(Some("Symbol evidence.".to_string())),
            ),
        ]),
        Some(vec!["kind".to_string()]),
        Some(false.into()),
    );
    let state_entry = JsonSchema::object(
        BTreeMap::from([
            (
                "key".to_string(),
                JsonSchema::string(Some(
                    "Stable key that owns this fact in exactly one memory layer.".to_string(),
                )),
            ),
            (
                "kind".to_string(),
                JsonSchema::string_enum(
                    vec![
                        serde_json::Value::String("sourceFact".to_string()),
                        serde_json::Value::String("userDecision".to_string()),
                        serde_json::Value::String("constraint".to_string()),
                        serde_json::Value::String("validation".to_string()),
                    ],
                    Some("Semantic state category.".to_string()),
                ),
            ),
            (
                "content".to_string(),
                JsonSchema::string(Some(
                    "Current effective fact, never a description of tool activity.".to_string(),
                )),
            ),
            (
                "evidence".to_string(),
                JsonSchema::array(
                    evidence,
                    Some("Evidence needed to verify or recall this state.".to_string()),
                ),
            ),
        ]),
        Some(vec![
            "key".to_string(),
            "kind".to_string(),
            "content".to_string(),
        ]),
        Some(false.into()),
    );
    let active_state = JsonSchema::object(
        BTreeMap::from([
            (
                "objective".to_string(),
                JsonSchema::string(Some("Current user-visible objective.".to_string())),
            ),
            (
                "entries".to_string(),
                JsonSchema::array(
                    state_entry.clone(),
                    Some("Precise facts needed for the next action.".to_string()),
                ),
            ),
            (
                "constraints".to_string(),
                string_array("Task-specific constraints not already present in memory."),
            ),
            (
                "openQuestions".to_string(),
                string_array("Only unresolved questions that can change the next action."),
            ),
            (
                "nextAction".to_string(),
                JsonSchema::string(Some("The next direct action.".to_string())),
            ),
        ]),
        Some(vec!["objective".to_string(), "nextAction".to_string()]),
        Some(false.into()),
    );
    let state = JsonSchema::object(
        BTreeMap::from([
            ("active".to_string(), active_state),
            (
                "sessionUpserts".to_string(),
                JsonSchema::array(
                    state_entry,
                    Some("Reusable session facts to insert or replace by key.".to_string()),
                ),
            ),
            (
                "forgetKeys".to_string(),
                string_array(
                    "Stale session or quarter keys to remove from the current projection.",
                ),
            ),
            (
                "quarterPromotions".to_string(),
                string_array(
                    "Existing session keys stable enough to promote when MEMORY_STATUS reports quarter_consolidation_due=true.",
                ),
            ),
        ]),
        Some(vec!["active".to_string()]),
        Some(false.into()),
    );
    let properties = BTreeMap::from([
        (
            "completedToolGroups".to_string(),
            string_array("Contiguous settled ToolGroup ids whose raw history can be replaced."),
        ),
        ("state".to_string(), state),
    ]);
    ToolSpec::Function(ResponsesApiTool {
        name: "update_context_state".to_string(),
        description: "Replace completed tool history with current effective Active, Session, and Quarter state. Do not describe tool activity or duplicate current source files."
            .to_string(),
        output_schema: None,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec!["completedToolGroups".to_string(), "state".to_string()]),
            Some(false.into()),
        ),
    })
}

fn recall_checkpoint_artifact_spec() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: "recall_checkpoint_artifact".to_string(),
        description: "Read a bounded, hash-verified artifact referenced by this session."
            .to_string(),
        output_schema: None,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::from([
                (
                    "artifactId".to_string(),
                    JsonSchema::string(Some("Artifact SHA-256 id.".to_string())),
                ),
                (
                    "maxBytes".to_string(),
                    JsonSchema::integer(Some(
                        "Maximum bytes to return; capped by Runtime.".to_string(),
                    )),
                ),
            ]),
            Some(vec!["artifactId".to_string()]),
            Some(false.into()),
        ),
    })
}

fn default_recall_bytes() -> usize {
    32 * 1024
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
