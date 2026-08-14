use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_context_checkpoint::ArtifactId;
use codex_context_checkpoint::RecallRequest;
use codex_context_checkpoint::UpdateSummaryRequest;
use codex_tools::JsonSchema;
use codex_tools::ResponsesApiTool;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

pub struct UpdateSummaryHandler;

impl ToolExecutor<ToolInvocation> for UpdateSummaryHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("update_summary")
    }

    fn spec(&self) -> ToolSpec {
        update_summary_spec()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(async move {
            let ToolPayload::Function { arguments } = invocation.payload else {
                return Err(FunctionCallError::RespondToModel(
                    "update_summary received an unsupported payload".to_string(),
                ));
            };
            let request =
                serde_json::from_str::<UpdateSummaryRequest>(&arguments).map_err(|error| {
                    FunctionCallError::RespondToModel(format!(
                        "failed to parse update_summary arguments: {error}"
                    ))
                })?;
            let pending = invocation
                .session
                .services
                .context_checkpoint
                .prepare_summary(request)
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

impl CoreToolRuntime for UpdateSummaryHandler {}

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

fn update_summary_spec() -> ToolSpec {
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
                        "workspacePath".to_string(),
                        "symbol".to_string(),
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
    let properties = BTreeMap::from([
        (
            "completedToolGroups".to_string(),
            string_array("Contiguous settled ToolGroup ids to freeze."),
        ),
        (
            "summary".to_string(),
            JsonSchema::string(Some("Purpose, key process, and actual result.".to_string())),
        ),
        (
            "evidence".to_string(),
            JsonSchema::array(
                evidence,
                Some("Verifiable evidence references.".to_string()),
            ),
        ),
        ("changes".to_string(), string_array("Actual changes made.")),
        (
            "validation".to_string(),
            string_array("Validation methods and real outcomes."),
        ),
        (
            "decisions".to_string(),
            string_array("User and engineering decisions."),
        ),
        (
            "openItems".to_string(),
            string_array("Unfinished items and why they remain open."),
        ),
        (
            "nextAction".to_string(),
            JsonSchema::string(Some("The next direct action.".to_string())),
        ),
        (
            "correctionOf".to_string(),
            JsonSchema::string(Some("Optional prior TurnRecord id to correct.".to_string())),
        ),
    ]);
    ToolSpec::Function(ResponsesApiTool {
        name: "update_summary".to_string(),
        description: "Freeze a completed, contiguous tool-work prefix into a durable checkpoint."
            .to_string(),
        output_schema: None,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec![
                "completedToolGroups".to_string(),
                "summary".to_string(),
                "nextAction".to_string(),
            ]),
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
