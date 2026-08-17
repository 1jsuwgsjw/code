use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_context_checkpoint::ArtifactId;
use codex_context_checkpoint::ArtifactLocator;
use codex_context_checkpoint::ArtifactRecallSelection;
use codex_context_checkpoint::ArtifactRecallView;
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
                "checkpointRef": codex_context_checkpoint::checkpoint_reference(
                    pending.record.generation_id,
                    pending.record.record_id,
                ),
                "recordId": pending.record.record_id.to_string(),
                "generationId": pending.record.generation_id.to_string(),
                "historyStart": pending.history_start,
                "historyEnd": pending.history_end,
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
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    mode: RecallMode,
    #[serde(default)]
    start_line: Option<usize>,
    #[serde(default)]
    end_line: Option<usize>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default = "default_search_context_lines")]
    context_lines: usize,
    #[serde(default = "default_search_matches")]
    max_matches: usize,
    #[serde(default = "default_recall_bytes")]
    max_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RecallMode {
    #[default]
    Outline,
    Lines,
    Search,
    Prefix,
}

impl RecallArguments {
    fn selection(&self) -> Result<ArtifactRecallSelection, String> {
        match self.mode {
            RecallMode::Outline => Ok(ArtifactRecallSelection::Outline),
            RecallMode::Lines => {
                let start_line = self
                    .start_line
                    .ok_or_else(|| "lines mode requires startLine".to_string())?;
                let end_line = self.end_line.unwrap_or_else(|| {
                    start_line.saturating_add(default_line_window().saturating_sub(1))
                });
                Ok(ArtifactRecallSelection::Lines {
                    start_line,
                    end_line,
                })
            }
            RecallMode::Search => Ok(ArtifactRecallSelection::Search {
                query: self
                    .query
                    .clone()
                    .ok_or_else(|| "search mode requires query".to_string())?,
                context_lines: self.context_lines,
                max_matches: self.max_matches,
            }),
            RecallMode::Prefix => Ok(ArtifactRecallSelection::Prefix),
        }
    }
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
            let locator = match (&arguments.reference, &arguments.artifact_id) {
                (Some(reference), None) => ArtifactLocator::Reference(reference.clone()),
                (None, Some(artifact_id)) => {
                    ArtifactLocator::ArtifactId(ArtifactId::from_sha256(artifact_id.clone()))
                }
                _ => {
                    return Err(FunctionCallError::RespondToModel(
                        "provide exactly one of reference or artifactId".to_string(),
                    ));
                }
            };
            let selection = arguments
                .selection()
                .map_err(FunctionCallError::RespondToModel)?;
            let recalled = invocation
                .session
                .services
                .context_checkpoint
                .recall(RecallRequest {
                    locator,
                    selection,
                    max_bytes: arguments.max_bytes,
                })
                .await
                .map_err(|error| FunctionCallError::RespondToModel(error.to_string()))?;
            let view = match recalled.view {
                ArtifactRecallView::Outline {
                    line_count,
                    sections,
                    truncated,
                    next_start_line,
                } => json!({
                    "mode": "outline",
                    "lineCount": line_count,
                    "sections": sections,
                    "truncated": truncated,
                    "nextStartLine": next_start_line,
                }),
                ArtifactRecallView::Lines {
                    line_count,
                    start_line,
                    end_line,
                    content,
                    truncated,
                    next_start_line,
                } => json!({
                    "mode": "lines",
                    "lineCount": line_count,
                    "startLine": start_line,
                    "endLine": end_line,
                    "content": content,
                    "truncated": truncated,
                    "nextStartLine": next_start_line,
                }),
                ArtifactRecallView::Search {
                    line_count,
                    query,
                    matches,
                    total_matches,
                    truncated,
                } => json!({
                    "mode": "search",
                    "lineCount": line_count,
                    "query": query,
                    "matches": matches,
                    "totalMatches": total_matches,
                    "truncated": truncated,
                }),
                ArtifactRecallView::Prefix {
                    line_count,
                    content,
                    truncated,
                } => json!({
                    "mode": "prefix",
                    "lineCount": line_count,
                    "content": content,
                    "truncated": truncated,
                }),
            };
            let output = json!({
                "reference": arguments.reference,
                "artifactId": recalled.artifact.artifact_id.as_str(),
                "sha256": recalled.artifact.sha256,
                "mediaType": recalled.artifact.media_type,
                "byteLength": recalled.artifact.byte_len,
                "view": view,
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
                JsonSchema::string(Some(
                    "Recorded artifact SHA-256 id from an earlier visible evidence bridge. Do not invent or recover ids for the ToolGroups selected in this call; Runtime attaches their output artifacts from stateKeys automatically."
                        .to_string(),
                )),
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
                        serde_json::Value::String("sourceCoverage".to_string()),
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
                    "Current effective knowledge. For sourceCoverage, name inspected symbols or ranges and explicit gaps; never merely say that a file was read or describe tool activity."
                        .to_string(),
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
                JsonSchema::string(Some(
                    "Current unfinished user-visible objective. Use an empty string only with an otherwise empty Active object to clear Active after semantic closure."
                        .to_string(),
                )),
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
                string_array("Unresolved questions whose answers can change future reasoning."),
            ),
            (
                "continuityHints".to_string(),
                string_array(
                    "Non-authoritative knowledge or evidence that may matter later. Never put actions, commitments, or a next step here.",
                ),
            ),
        ]),
        Some(vec!["objective".to_string()]),
        Some(false.into()),
    );
    let state_removal = JsonSchema::object(
        BTreeMap::from([
            (
                "key".to_string(),
                JsonSchema::string(Some("Existing session or quarter key to retire.".to_string())),
            ),
            (
                "reason".to_string(),
                JsonSchema::string_enum(
                    vec![
                        serde_json::Value::String("userRevoked".to_string()),
                        serde_json::Value::String("superseded".to_string()),
                    ],
                    Some(
                        "Removal authority. Source revision invalidation is automatic and must not be requested here."
                            .to_string(),
                    ),
                ),
            ),
            (
                "supersededBy".to_string(),
                JsonSchema::string(Some(
                    "Retained replacement state key; required only when reason=superseded."
                        .to_string(),
                )),
            ),
        ]),
        Some(vec!["key".to_string(), "reason".to_string()]),
        Some(false.into()),
    );
    let state = JsonSchema::object(
        BTreeMap::from([
            ("active".to_string(), active_state),
            (
                "sessionUpserts".to_string(),
                JsonSchema::array(
                    state_entry,
                    Some(
                        "Reusable project knowledge to insert or replace by key. Put completed, verified facts here instead of keeping them in Active. Never store tool usage, checkpoint bookkeeping, task lifecycle narration, or facts already supplied by higher-priority instructions."
                            .to_string(),
                    ),
                ),
            ),
            (
                "removals".to_string(),
                JsonSchema::array(
                    state_removal,
                    Some(
                        "Explicitly justified retirements. Bare forgetting is not permitted."
                            .to_string(),
                    ),
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
    let tool_group_settlement = JsonSchema::object(
        BTreeMap::from([
            (
                "groupId".to_string(),
                JsonSchema::string(Some("Completed ToolGroup id.".to_string())),
            ),
            (
                "disposition".to_string(),
                JsonSchema::string_enum(
                    vec![
                        serde_json::Value::String("promote".to_string()),
                        serde_json::Value::String("keepOpen".to_string()),
                        serde_json::Value::String("archiveOnly".to_string()),
                    ],
                    Some(
                        "promote links the group to effective state, keepOpen links it to unresolved information, and archiveOnly removes process history from the prompt while immutable artifacts remain externally recallable. Truncation or a tool error alone does not require promotion."
                            .to_string(),
                    ),
                ),
            ),
            (
                "stateKeys".to_string(),
                string_array(
                    "Retained state keys produced or confirmed by this group. Required for promote. Runtime automatically attaches a bounded set of this group's newest output artifacts to those entries, so the model does not supply their artifact ids.",
                ),
            ),
            (
                "openQuestions".to_string(),
                string_array(
                    "Exact active openQuestions preserved by this group. Required for keepOpen.",
                ),
            ),
        ]),
        Some(vec!["groupId".to_string(), "disposition".to_string()]),
        Some(false.into()),
    );
    let properties = BTreeMap::from([
        (
            "completedToolGroups".to_string(),
            string_array(
                "A contiguous settled ToolGroup prefix selected only when a semantic work stage has closed, context pressure requires compaction, or the user explicitly requested a checkpoint. Do not checkpoint merely because tools were called.",
            ),
        ),
        (
            "toolGroupSettlements".to_string(),
            JsonSchema::array(
                tool_group_settlement,
                Some(
                    "Exactly one ordered semantic settlement for each selected ToolGroup. Archive process noise instead of inventing state entries to justify compression."
                        .to_string(),
                ),
            ),
        ),
        ("state".to_string(), state),
    ]);
    ToolSpec::Function(ResponsesApiTool {
        name: "update_context_state".to_string(),
        description: "Create a checkpoint only at semantic stage closure, under context pressure, or on explicit user request; never use it as routine bookkeeping after tool calls. Replace selected completed tool history with current effective knowledge, unresolved information, source coverage, and direct evidence bridges. Compression is not task planning and never chooses a next action. Keep only unfinished work in Active, move completed reusable facts to Session, clear Active when no work remains, and archive tool usage, task lifecycle narration, checkpoint bookkeeping, and other process noise even when its immutable artifacts remain recallable. Associate promoted evidence through toolGroupSettlements.stateKeys; Runtime attaches bounded output-artifact references to those entries automatically, so never invent or memorize artifact ids for the selected groups."
            .to_string(),
        output_schema: None,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            Some(vec![
                "completedToolGroups".to_string(),
                "toolGroupSettlements".to_string(),
                "state".to_string(),
            ]),
            Some(false.into()),
        ),
    })
}

fn recall_checkpoint_artifact_spec() -> ToolSpec {
    ToolSpec::Function(ResponsesApiTool {
        name: "recall_checkpoint_artifact".to_string(),
        description: "Inspect a hash-verified checkpoint artifact without reinjecting the whole tool result. Default mode=outline returns line ranges and previews; use mode=lines for a selected range or mode=search for bounded keyword matches. Prefer the short reference rendered beside the retained knowledge that the artifact supports; Runtime generates that reference from stateKeys, so the model never needs to memorize a SHA-256 id. Provide exactly one of reference or legacy artifactId."
            .to_string(),
        output_schema: None,
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            BTreeMap::from([
                (
                    "reference".to_string(),
                    JsonSchema::string(Some(
                        "Semantic artifact reference, for example artifact:TR000001/A001."
                            .to_string(),
                    )),
                ),
                (
                    "artifactId".to_string(),
                    JsonSchema::string(Some(
                        "Legacy artifact SHA-256 id. Prefer reference when available.".to_string(),
                    )),
                ),
                (
                    "mode".to_string(),
                    JsonSchema::string_enum(
                        vec![
                            json!("outline"),
                            json!("lines"),
                            json!("search"),
                            json!("prefix"),
                        ],
                        Some(
                            "Recall mode. Defaults to outline. Prefix is legacy and should be used only when exact leading bytes are required."
                                .to_string(),
                        ),
                    ),
                ),
                (
                    "startLine".to_string(),
                    JsonSchema::integer(Some(
                        "First 1-based line for mode=lines.".to_string(),
                    )),
                ),
                (
                    "endLine".to_string(),
                    JsonSchema::integer(Some(
                        "Last 1-based line for mode=lines; defaults to a 40-line window."
                            .to_string(),
                    )),
                ),
                (
                    "query".to_string(),
                    JsonSchema::string(Some(
                        "Keyword or phrase for mode=search.".to_string(),
                    )),
                ),
                (
                    "contextLines".to_string(),
                    JsonSchema::integer(Some(
                        "Neighboring lines returned around each search match; defaults to 2 and is capped."
                            .to_string(),
                    )),
                ),
                (
                    "maxMatches".to_string(),
                    JsonSchema::integer(Some(
                        "Maximum search matches to return; defaults to 8 and is capped."
                            .to_string(),
                    )),
                ),
                (
                    "maxBytes".to_string(),
                    JsonSchema::integer(Some(
                        "Maximum bytes for previews or selected content; defaults to 8192 and is capped by Runtime."
                            .to_string(),
                    )),
                ),
            ]),
            /*required*/ None,
            Some(false.into()),
        ),
    })
}

fn default_recall_bytes() -> usize {
    8 * 1024
}

fn default_line_window() -> usize {
    40
}

fn default_search_context_lines() -> usize {
    2
}

fn default_search_matches() -> usize {
    8
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
