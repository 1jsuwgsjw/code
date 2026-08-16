use super::Session;
use crate::state::TurnRuntimeSnapshot;
use crate::state::WorkflowMessageCandidate;
use crate::state::WorkflowMessageKind;
use crate::state::WorkflowOperationOutcome;
use crate::state::WorkflowRuntimeEvent;
use crate::state::WorkflowRuntimeState;
use codex_context_checkpoint::content_sha256;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WorldStateItem;
use std::sync::Arc;

impl Session {
    pub(super) async fn workflow_runtime_world_state_item_for_event(
        &self,
        message: &EventMsg,
    ) -> Option<WorldStateItem> {
        let event = match message {
            EventMsg::TurnStarted(event) => WorkflowRuntimeEvent::TurnStarted {
                turn_id: event.turn_id.clone(),
            },
            EventMsg::TurnComplete(event) => WorkflowRuntimeEvent::TurnFinished {
                turn_id: Some(event.turn_id.clone()),
                outcome: WorkflowOperationOutcome::Completed,
            },
            EventMsg::TurnAborted(event) => WorkflowRuntimeEvent::TurnFinished {
                turn_id: event.turn_id.clone(),
                outcome: WorkflowOperationOutcome::Aborted,
            },
            _ => WorkflowRuntimeEvent::Snapshot,
        };
        let runtime = if matches!(&event, WorkflowRuntimeEvent::TurnFinished { .. }) {
            TurnRuntimeSnapshot::default()
        } else {
            self.workflow_runtime_turn_snapshot().await
        };
        let task_id = self.thread_id.to_string();
        let mut state = self.state.lock().await;
        if !state.workflow_runtime.apply_event(&task_id, event, runtime) {
            return None;
        }
        workflow_runtime_world_state_item(&state.workflow_runtime)
    }

    pub(super) async fn workflow_runtime_world_state_item_for_messages(
        &self,
        items: &[ResponseItem],
    ) -> Option<WorldStateItem> {
        let messages = items
            .iter()
            .filter_map(workflow_message_candidate)
            .collect::<Vec<_>>();
        if messages.is_empty() {
            return None;
        }
        let task_id = self.thread_id.to_string();
        let mut state = self.state.lock().await;
        if !state.workflow_runtime.register_messages(&task_id, messages) {
            return None;
        }
        workflow_runtime_world_state_item(&state.workflow_runtime)
    }

    pub(super) async fn workflow_runtime_turn_snapshot(&self) -> TurnRuntimeSnapshot {
        let turn_state = {
            let active_turn = self.active_turn.lock().await;
            active_turn
                .as_ref()
                .map(|active_turn| Arc::clone(&active_turn.turn_state))
        };
        match turn_state {
            Some(turn_state) => turn_state.lock().await.workflow_runtime_snapshot(),
            None => TurnRuntimeSnapshot::default(),
        }
    }
}

fn workflow_runtime_world_state_item(state: &WorkflowRuntimeState) -> Option<WorldStateItem> {
    match state.world_state_merge_patch() {
        Ok(patch) => Some(WorldStateItem::patch(patch)),
        Err(error) => {
            tracing::warn!(error = %error, "failed to serialize workflow runtime state");
            None
        }
    }
}

fn workflow_message_candidate(item: &ResponseItem) -> Option<WorkflowMessageCandidate> {
    let (role, phase) = match item {
        ResponseItem::Message { role, phase, .. } => (role.as_str(), phase),
        _ => return None,
    };
    let kind = match role {
        "user" => WorkflowMessageKind::User,
        "assistant" if !matches!(phase, Some(MessagePhase::Commentary)) => {
            WorkflowMessageKind::AssistantFinal
        }
        _ => return None,
    };
    let serialized = serde_json::to_vec(item).ok()?;
    Some(WorkflowMessageCandidate {
        source_item_id: item.id().map(ToString::to_string),
        turn_id: item.turn_id().map(str::to_owned),
        kind,
        content_sha256: content_sha256(&serialized),
    })
}
