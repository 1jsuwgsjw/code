use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::plan_spec::create_update_plan_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::config_types::ModeKind;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::plan_tool::ResearchStateUpdate;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::EventMsg;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde_json::Value as JsonValue;

pub struct PlanHandler;

pub struct PlanToolOutput {
    research_update: Option<ResearchStateUpdate>,
}

const PLAN_UPDATED_MESSAGE: &str = "Plan updated";

impl PlanToolOutput {
    fn response_message(&self) -> String {
        let Some(update) = &self.research_update else {
            return PLAN_UPDATED_MESSAGE.to_string();
        };
        let change = if update.changed {
            "changed"
        } else {
            "unchanged"
        };
        let entry_count = update.entries.len();
        let entry_label = if entry_count == 1 { "entry" } else { "entries" };
        format!(
            "{PLAN_UPDATED_MESSAGE}; research state revision {} ({change}, {entry_count} {entry_label})",
            update.revision
        )
    }
}

impl ToolOutput for PlanToolOutput {
    fn log_preview(&self) -> String {
        self.response_message()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        let mut output = FunctionCallOutputPayload::from_text(self.response_message());
        output.success = Some(true);

        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output,
        }
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        let Some(update) = &self.research_update else {
            return JsonValue::Object(serde_json::Map::new());
        };
        serde_json::json!({
            "research_state": {
                "revision": update.revision,
                "changed": update.changed,
                "entry_count": update.entries.len(),
            }
        })
    }
}

impl ToolExecutor<ToolInvocation> for PlanHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("update_plan")
    }

    fn spec(&self) -> ToolSpec {
        create_update_plan_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl PlanHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            call_id: _,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "update_plan handler received unsupported payload".to_string(),
                ));
            }
        };

        if turn.collaboration_mode.mode == ModeKind::Plan {
            return Err(FunctionCallError::RespondToModel(
                "update_plan is a TODO/checklist tool and is not allowed in Plan mode".to_string(),
            ));
        }

        let args = parse_update_plan_arguments(&arguments)?;
        let research_update = if let Some(research_delta) = args.research_delta.as_deref() {
            let update = session
                .apply_research_delta(research_delta)
                .await
                .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
            tracing::info!(
                target: "codex_research_state",
                revision = update.revision,
                changed = update.changed,
                entry_count = update.entries.len(),
                delta_count = research_delta.len(),
                "research state delta applied"
            );
            Some(update)
        } else {
            None
        };
        session
            .send_event(turn.as_ref(), EventMsg::PlanUpdate(args))
            .await;

        if let Some(update) = research_update.as_ref() {
            session
                .send_event(
                    turn.as_ref(),
                    EventMsg::ResearchStateUpdated(update.clone()),
                )
                .await;
        }

        Ok(boxed_tool_output(PlanToolOutput { research_update }))
    }
}

impl CoreToolRuntime for PlanHandler {}

fn parse_update_plan_arguments(arguments: &str) -> Result<UpdatePlanArgs, FunctionCallError> {
    serde_json::from_str::<UpdatePlanArgs>(arguments).map_err(|e| {
        FunctionCallError::RespondToModel(format!("failed to parse function arguments: {e}"))
    })
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
