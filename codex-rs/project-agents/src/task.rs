use crate::ProjectAgentId;
use crate::ProjectAgentValidationError;
use crate::types::validate_bounded_text;
use crate::types::validate_schema;
use crate::types::validate_task_id;
use serde::Deserialize;
use serde::Serialize;

const MAX_TASK_BYTES: usize = 16 * 1024;
pub(crate) const MAX_THREAD_ID_BYTES: usize = 128;
const MAX_TASK_LIST_ITEMS: usize = 100;
const DEFAULT_TASK_LIST_ITEMS: usize = 25;

/// Lifecycle phase for a project AGENT task outside the model-authored result protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAgentTaskPhase {
    Queued,
    Running,
    Completed,
    Interrupted,
    Failed,
}

impl ProjectAgentTaskPhase {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

/// Bounded metadata used to display and control one project AGENT task.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentTaskMetadata {
    pub schema_version: u32,
    pub agent_id: ProjectAgentId,
    pub task_id: String,
    pub task: String,
    pub phase: ProjectAgentTaskPhase,
    pub session_thread_id: Option<String>,
    pub parent_thread_id: String,
    pub attempt: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ProjectAgentTaskMetadata {
    pub fn validate_for(
        &self,
        expected_agent_id: &ProjectAgentId,
        expected_task_id: &str,
    ) -> Result<(), ProjectAgentValidationError> {
        validate_schema("task metadata", self.schema_version)?;
        if &self.agent_id != expected_agent_id {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "agent_id",
                reason: format!(
                    "task metadata names `{}`, expected `{expected_agent_id}`",
                    self.agent_id
                ),
            });
        }
        if self.task_id != expected_task_id {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "task_id",
                reason: format!(
                    "task metadata names `{}`, expected `{expected_task_id}`",
                    self.task_id
                ),
            });
        }
        validate_task_id(&self.task_id)?;
        validate_bounded_text("task", &self.task, MAX_TASK_BYTES, /*required*/ true)?;
        if let Some(thread_id) = self.session_thread_id.as_deref() {
            validate_thread_id("session_thread_id", thread_id)?;
        }
        validate_thread_id("parent_thread_id", &self.parent_thread_id)?;
        if self.attempt == 0 {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "attempt",
                reason: "must be at least 1".to_string(),
            });
        }
        validate_timestamps(self.created_at, self.updated_at)
    }
}

/// Durable identity of the reusable Codex thread assigned to one project AGENT.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentSessionMetadata {
    pub schema_version: u32,
    pub agent_id: ProjectAgentId,
    pub thread_id: String,
    pub generation: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ProjectAgentSessionMetadata {
    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("session metadata", self.schema_version)?;
        validate_thread_id("thread_id", &self.thread_id)?;
        if self.generation == 0 {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "generation",
                reason: "must be at least 1".to_string(),
            });
        }
        validate_timestamps(self.created_at, self.updated_at)
    }
}

/// Self-documenting bound for recent task-history reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectAgentTaskListLimit(usize);

impl ProjectAgentTaskListLimit {
    pub fn new(value: usize) -> Self {
        Self(value.clamp(1, MAX_TASK_LIST_ITEMS))
    }

    pub(crate) fn get(self) -> usize {
        self.0
    }
}

impl Default for ProjectAgentTaskListLimit {
    fn default() -> Self {
        Self(DEFAULT_TASK_LIST_ITEMS)
    }
}

pub(crate) fn validate_thread_id(
    field: &'static str,
    value: &str,
) -> Result<(), ProjectAgentValidationError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_THREAD_ID_BYTES
        && value.chars().all(|character| !character.is_control());
    if valid {
        Ok(())
    } else {
        Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("must be non-empty and at most {MAX_THREAD_ID_BYTES} bytes"),
        })
    }
}

fn validate_timestamps(
    created_at: i64,
    updated_at: i64,
) -> Result<(), ProjectAgentValidationError> {
    if created_at < 0 || updated_at < created_at {
        return Err(ProjectAgentValidationError::InvalidField {
            field: "updated_at",
            reason: "timestamps must be non-negative and updated_at must not precede created_at"
                .to_string(),
        });
    }
    Ok(())
}
