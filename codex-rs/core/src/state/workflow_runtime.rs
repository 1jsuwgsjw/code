use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const WORKFLOW_RUNTIME_WORLD_STATE_ID: &str = "workflow_runtime";

const MAX_COMPLETED_OPERATIONS: usize = 32;
const MAX_MESSAGE_REFERENCES: usize = 64;
const MAX_EXTERNAL_WAITS: usize = 32;
const MAX_STORED_ID_CHARS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct WorkflowRuntimeState {
    task_id: String,
    task_revision: u64,
    next_operation_sequence: u64,
    next_message_sequence: u64,
    active_operation: Option<ActiveWorkflowOperation>,
    completed_operations: Vec<CompletedWorkflowOperation>,
    message_references: Vec<WorkflowMessageReference>,
    external_waits: Vec<WorkflowExternalWait>,
    queued_input_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveWorkflowOperation {
    operation_id: String,
    turn_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompletedWorkflowOperation {
    operation_id: String,
    turn_id: String,
    outcome: WorkflowOperationOutcome,
    completed_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkflowOperationOutcome {
    Completed,
    Aborted,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkflowRuntimeEvent {
    Snapshot,
    TurnStarted {
        turn_id: String,
    },
    TurnFinished {
        turn_id: Option<String>,
        outcome: WorkflowOperationOutcome,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkflowMessageKind {
    User,
    AssistantFinal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkflowMessageCandidate {
    pub(crate) source_item_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) kind: WorkflowMessageKind,
    pub(crate) content_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowMessageReference {
    reference_id: String,
    source_item_id: Option<String>,
    turn_id: Option<String>,
    operation_id: Option<String>,
    kind: WorkflowMessageKind,
    content_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExternalWaitKind {
    Approval,
    RequestPermissions,
    RequestUserInput,
    McpElicitation,
    DynamicTool,
    QueuedInput,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PendingExternalWait {
    pub(crate) kind: ExternalWaitKind,
    pub(crate) source_id: String,
}

impl PendingExternalWait {
    pub(crate) fn new(kind: ExternalWaitKind, source_id: impl Into<String>) -> Self {
        Self {
            kind,
            source_id: source_id.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnRuntimeSnapshot {
    pub(crate) pending_waits: Vec<PendingExternalWait>,
    pub(crate) queued_input_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ExternalWaitStatus {
    Pending,
    Terminated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowExternalWait {
    kind: ExternalWaitKind,
    source_id: String,
    operation_id: Option<String>,
    status: ExternalWaitStatus,
    termination_reason: Option<String>,
}

impl WorkflowRuntimeState {
    pub(crate) fn apply_event(
        &mut self,
        task_id: &str,
        event: WorkflowRuntimeEvent,
        runtime: TurnRuntimeSnapshot,
    ) -> bool {
        let previous = self.clone();
        let next_revision = self.task_revision.saturating_add(1);
        self.ensure_task_identity(task_id);
        match event {
            WorkflowRuntimeEvent::Snapshot => {}
            WorkflowRuntimeEvent::TurnStarted { turn_id } => {
                self.begin_operation(turn_id, next_revision);
            }
            WorkflowRuntimeEvent::TurnFinished { turn_id, outcome } => {
                self.finish_operation(turn_id.as_deref(), outcome, next_revision);
            }
        }
        self.sync_runtime_snapshot(runtime);
        self.enforce_bounds();
        if previous == *self {
            return false;
        }
        self.task_revision = next_revision;
        true
    }

    pub(crate) fn register_messages(
        &mut self,
        task_id: &str,
        messages: Vec<WorkflowMessageCandidate>,
    ) -> bool {
        let previous = self.clone();
        self.ensure_task_identity(task_id);
        let operation_id = self
            .active_operation
            .as_ref()
            .map(|operation| operation.operation_id.clone());
        for message in messages {
            let source_item_id = message
                .source_item_id
                .map(|id| bounded_text(id, MAX_STORED_ID_CHARS));
            let turn_id = message
                .turn_id
                .map(|id| bounded_text(id, MAX_STORED_ID_CHARS));
            let content_sha256 = bounded_text(message.content_sha256, MAX_STORED_ID_CHARS);
            let duplicate = self.message_references.iter().any(|reference| {
                reference.source_item_id == source_item_id
                    && reference.turn_id == turn_id
                    && reference.kind == message.kind
                    && reference.content_sha256 == content_sha256
            });
            if duplicate {
                continue;
            }
            self.next_message_sequence = self.next_message_sequence.saturating_add(1);
            self.message_references.push(WorkflowMessageReference {
                reference_id: format!("MSG{:06}", self.next_message_sequence),
                source_item_id,
                turn_id,
                operation_id: operation_id.clone(),
                kind: message.kind,
                content_sha256,
            });
        }
        self.enforce_bounds();
        if previous == *self {
            return false;
        }
        self.task_revision = self.task_revision.saturating_add(1);
        true
    }

    pub(crate) fn reconcile_after_resume(&mut self, task_id: &str) -> bool {
        let previous = self.clone();
        let next_revision = self.task_revision.saturating_add(1);
        self.ensure_task_identity(task_id);
        if let Some(active) = self.active_operation.take() {
            self.push_completed(active, WorkflowOperationOutcome::Interrupted, next_revision);
        }
        for wait in &mut self.external_waits {
            if wait.status == ExternalWaitStatus::Pending {
                wait.status = ExternalWaitStatus::Terminated;
                wait.termination_reason =
                    Some("runtime channel unavailable after resume".to_string());
            }
        }
        if self.queued_input_count > 0 {
            self.external_waits.push(WorkflowExternalWait {
                kind: ExternalWaitKind::QueuedInput,
                source_id: format!("{} queued item(s)", self.queued_input_count),
                operation_id: None,
                status: ExternalWaitStatus::Terminated,
                termination_reason: Some(
                    "queued input was not recoverable after resume".to_string(),
                ),
            });
            self.queued_input_count = 0;
        }
        self.enforce_bounds();
        if previous == *self {
            return false;
        }
        self.task_revision = next_revision;
        true
    }

    pub(crate) fn task_revision(&self) -> u64 {
        self.task_revision
    }

    pub(crate) fn world_state_merge_patch(&self) -> serde_json::Result<Value> {
        serde_json::to_value(BTreeMap::from([(WORKFLOW_RUNTIME_WORLD_STATE_ID, self)]))
    }

    pub(crate) fn model_view(&self) -> String {
        let active = self
            .active_operation
            .as_ref()
            .map(|operation| operation.operation_id.as_str())
            .unwrap_or("none");
        let completed = self
            .completed_operations
            .iter()
            .map(|operation| format!("{} {:?}", operation.operation_id, operation.outcome))
            .collect::<Vec<_>>()
            .join("; ");
        let messages = self
            .message_references
            .iter()
            .map(|reference| format!("{} {:?}", reference.reference_id, reference.kind))
            .collect::<Vec<_>>()
            .join("; ");
        let waits = self
            .external_waits
            .iter()
            .map(|wait| format!("{:?}:{}:{:?}", wait.kind, wait.source_id, wait.status))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "authority: structured runtime state; summaries cannot override it\n\
             task: current thread\nrevision: {}\nactiveOperation: {active}\n\
             completedOperations({}): {completed}\nmessageReferences({}): {messages}\n\
             externalWaits({}): {waits}\nqueuedInputCount: {}",
            self.task_revision,
            self.completed_operations.len(),
            self.message_references.len(),
            self.external_waits.len(),
            self.queued_input_count,
        )
    }

    fn ensure_task_identity(&mut self, task_id: &str) {
        let task_id = bounded_text(task_id.to_string(), MAX_STORED_ID_CHARS);
        if self.task_id == task_id {
            return;
        }
        self.task_id = task_id;
    }

    fn begin_operation(&mut self, turn_id: String, revision: u64) {
        let turn_id = bounded_text(turn_id, MAX_STORED_ID_CHARS);
        if self.has_completed_turn(&turn_id)
            || self
                .active_operation
                .as_ref()
                .is_some_and(|operation| operation.turn_id == turn_id)
        {
            return;
        }
        if let Some(active) = self.active_operation.take() {
            self.push_completed(active, WorkflowOperationOutcome::Interrupted, revision);
        }
        let operation = self.new_operation(turn_id);
        self.active_operation = Some(operation);
    }

    fn finish_operation(
        &mut self,
        turn_id: Option<&str>,
        outcome: WorkflowOperationOutcome,
        revision: u64,
    ) {
        let turn_id = turn_id
            .filter(|turn_id| !turn_id.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                self.active_operation
                    .as_ref()
                    .map(|operation| operation.turn_id.clone())
            });
        let Some(turn_id) = turn_id else {
            return;
        };
        let turn_id = bounded_text(turn_id, MAX_STORED_ID_CHARS);
        if self.has_completed_turn(&turn_id) {
            return;
        }
        let operation = if self
            .active_operation
            .as_ref()
            .is_some_and(|operation| operation.turn_id == turn_id)
        {
            self.active_operation
                .take()
                .expect("active operation exists")
        } else {
            self.new_operation(turn_id)
        };
        self.push_completed(operation, outcome, revision);
    }

    fn new_operation(&mut self, turn_id: String) -> ActiveWorkflowOperation {
        self.next_operation_sequence = self.next_operation_sequence.saturating_add(1);
        ActiveWorkflowOperation {
            operation_id: format!("OP{:06}", self.next_operation_sequence),
            turn_id,
        }
    }

    fn has_completed_turn(&self, turn_id: &str) -> bool {
        self.completed_operations
            .iter()
            .any(|operation| operation.turn_id == turn_id)
    }

    fn push_completed(
        &mut self,
        operation: ActiveWorkflowOperation,
        outcome: WorkflowOperationOutcome,
        revision: u64,
    ) {
        self.completed_operations.push(CompletedWorkflowOperation {
            operation_id: operation.operation_id,
            turn_id: operation.turn_id,
            outcome,
            completed_revision: revision,
        });
    }

    fn sync_runtime_snapshot(&mut self, runtime: TurnRuntimeSnapshot) {
        let operation_id = self
            .active_operation
            .as_ref()
            .map(|operation| operation.operation_id.clone());
        let mut waits = self
            .external_waits
            .iter()
            .filter(|wait| wait.status == ExternalWaitStatus::Terminated)
            .cloned()
            .collect::<Vec<_>>();
        waits.extend(
            runtime
                .pending_waits
                .into_iter()
                .map(|wait| WorkflowExternalWait {
                    kind: wait.kind,
                    source_id: bounded_text(wait.source_id, MAX_STORED_ID_CHARS),
                    operation_id: operation_id.clone(),
                    status: ExternalWaitStatus::Pending,
                    termination_reason: None,
                }),
        );
        self.external_waits = waits;
        self.queued_input_count = runtime.queued_input_count;
    }

    fn enforce_bounds(&mut self) {
        retain_latest(&mut self.completed_operations, MAX_COMPLETED_OPERATIONS);
        retain_latest(&mut self.message_references, MAX_MESSAGE_REFERENCES);
        retain_latest(&mut self.external_waits, MAX_EXTERNAL_WAITS);
    }
}

fn retain_latest<T>(items: &mut Vec<T>, limit: usize) {
    if items.len() > limit {
        items.drain(..items.len() - limit);
    }
}

fn bounded_text(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut bounded = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    bounded.push('…');
    bounded
}
