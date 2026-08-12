use crate::PROJECT_AGENT_SCHEMA_VERSION;
use crate::ProjectAgentId;
use crate::ProjectAgentValidationError;
use crate::types::validate_bounded_text;
use crate::types::validate_schema;
use crate::types::validate_task_id;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::Error as _;
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

const MAX_TASKS: usize = 256;
const MAX_DEPTH: usize = 8;
const MAX_TITLE_BYTES: usize = 256;
const MAX_OBJECTIVE_BYTES: usize = 8 * 1024;
const MAX_REQUIREMENTS: usize = 32;
const MAX_RESULT_ITEMS: usize = 64;
const MAX_ITEM_BYTES: usize = 4 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024;
const MAX_SUGGESTIONS: usize = 16;
const MAX_EVALUATION_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectTaskId(String);

impl ProjectTaskId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectAgentValidationError> {
        let value = value.into();
        validate_task_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectTaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for ProjectTaskId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskStatus {
    Pending,
    InProgress,
    Completed,
    Rejected,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectTaskExecutor {
    Unassigned,
    MainAgent,
    ProjectAgent { agent_id: ProjectAgentId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskResultStatus {
    Completed,
    Rejected,
    BlockedMissingTool,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectTaskSuggestedChild {
    pub title: String,
    pub objective: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectTaskResult {
    pub status: ProjectTaskResultStatus,
    pub summary: String,
    pub artifacts: Vec<String>,
    pub evidence: Vec<String>,
    pub suggested_children: Vec<ProjectTaskSuggestedChild>,
    pub error: Option<String>,
    pub completed_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskEvaluationVerdict {
    Passed,
    NeedsRevision,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectTaskEvaluation {
    pub verdict: ProjectTaskEvaluationVerdict,
    pub summary: String,
    pub evidence: Vec<String>,
    pub evaluated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectTaskNode {
    pub task_id: ProjectTaskId,
    pub parent_task_id: Option<ProjectTaskId>,
    pub title: String,
    pub objective: String,
    pub requirements: Vec<String>,
    pub status: ProjectTaskStatus,
    pub executor: ProjectTaskExecutor,
    pub execution_task_id: Option<String>,
    pub result: Option<ProjectTaskResult>,
    pub evaluation: Option<ProjectTaskEvaluation>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectTaskWorkspace {
    pub schema_version: u32,
    pub tasks: Vec<ProjectTaskNode>,
    pub updated_at: i64,
}

impl Default for ProjectTaskWorkspace {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            tasks: Vec::new(),
            updated_at: 0,
        }
    }
}

impl ProjectTaskWorkspace {
    pub fn task(&self, task_id: &ProjectTaskId) -> Option<&ProjectTaskNode> {
        self.tasks.iter().find(|task| task.task_id == *task_id)
    }

    pub fn root_tasks(&self) -> impl Iterator<Item = &ProjectTaskNode> {
        self.tasks
            .iter()
            .filter(|task| task.parent_task_id.is_none())
    }

    pub fn child_tasks<'a>(
        &'a self,
        parent_task_id: &'a ProjectTaskId,
    ) -> impl Iterator<Item = &'a ProjectTaskNode> + 'a {
        self.tasks
            .iter()
            .filter(move |task| task.parent_task_id.as_ref() == Some(parent_task_id))
    }

    pub fn create_task(
        &mut self,
        task_id: ProjectTaskId,
        parent_task_id: Option<ProjectTaskId>,
        title: String,
        objective: String,
        executor: ProjectTaskExecutor,
        created_at: i64,
    ) -> Result<(), ProjectTaskWorkspaceError> {
        if self.tasks.len() == MAX_TASKS {
            return Err(ProjectTaskWorkspaceError::Limit("tasks"));
        }
        if self.task(&task_id).is_some() {
            return Err(ProjectTaskWorkspaceError::TaskAlreadyExists(task_id));
        }
        if let Some(parent_id) = parent_task_id.as_ref() {
            if self.depth(parent_id)? == MAX_DEPTH {
                return Err(ProjectTaskWorkspaceError::Limit("task depth"));
            }
        }
        validate_bounded_text("title", &title, MAX_TITLE_BYTES, /*required*/ true)?;
        validate_bounded_text(
            "objective",
            &objective,
            MAX_OBJECTIVE_BYTES,
            /*required*/ true,
        )?;
        validate_timestamp(created_at)?;
        let status = match &executor {
            ProjectTaskExecutor::MainAgent => ProjectTaskStatus::InProgress,
            ProjectTaskExecutor::Unassigned | ProjectTaskExecutor::ProjectAgent { .. } => {
                ProjectTaskStatus::Pending
            }
        };
        self.tasks.push(ProjectTaskNode {
            task_id,
            parent_task_id,
            title,
            objective,
            requirements: Vec::new(),
            status,
            executor,
            execution_task_id: None,
            result: None,
            evaluation: None,
            created_at,
            updated_at: created_at,
        });
        self.updated_at = self.updated_at.max(created_at);
        Ok(())
    }

    pub fn append_requirement(
        &mut self,
        task_id: &ProjectTaskId,
        requirement: String,
        updated_at: i64,
    ) -> Result<(), ProjectTaskWorkspaceError> {
        let task = self.task_mut(task_id)?;
        ensure_active(task, "append a requirement")?;
        if task.requirements.len() == MAX_REQUIREMENTS {
            return Err(ProjectTaskWorkspaceError::Limit("requirements"));
        }
        validate_update(task, updated_at)?;
        validate_bounded_text(
            "requirement",
            &requirement,
            MAX_ITEM_BYTES,
            /*required*/ true,
        )?;
        task.requirements.push(requirement);
        task.updated_at = updated_at;
        self.updated_at = self.updated_at.max(updated_at);
        Ok(())
    }

    pub fn start_execution(
        &mut self,
        task_id: &ProjectTaskId,
        agent_id: ProjectAgentId,
        execution_task_id: String,
        updated_at: i64,
    ) -> Result<(), ProjectTaskWorkspaceError> {
        let task = self.task_mut(task_id)?;
        if task.status != ProjectTaskStatus::Pending {
            return Err(invalid_state(task, "start execution"));
        }
        validate_update(task, updated_at)?;
        validate_task_id(&execution_task_id)?;
        task.executor = ProjectTaskExecutor::ProjectAgent { agent_id };
        task.execution_task_id = Some(execution_task_id);
        task.status = ProjectTaskStatus::InProgress;
        task.updated_at = updated_at;
        self.updated_at = self.updated_at.max(updated_at);
        Ok(())
    }

    pub fn record_result(
        &mut self,
        task_id: &ProjectTaskId,
        result: ProjectTaskResult,
    ) -> Result<(), ProjectTaskWorkspaceError> {
        let task = self.task_mut(task_id)?;
        ensure_active(task, "record a result")?;
        validate_update(task, result.completed_at)?;
        result.validate()?;
        task.status = result.status.into();
        task.updated_at = result.completed_at;
        task.result = Some(result);
        task.evaluation = None;
        let updated_at = task.updated_at;
        self.updated_at = self.updated_at.max(updated_at);
        Ok(())
    }

    pub fn set_evaluation(
        &mut self,
        task_id: &ProjectTaskId,
        evaluation: ProjectTaskEvaluation,
    ) -> Result<(), ProjectTaskWorkspaceError> {
        let task = self.task_mut(task_id)?;
        if task.result.is_none() {
            return Err(invalid_state(task, "evaluate without a result"));
        }
        validate_update(task, evaluation.evaluated_at)?;
        evaluation.validate()?;
        task.updated_at = evaluation.evaluated_at;
        task.evaluation = Some(evaluation);
        let updated_at = task.updated_at;
        self.updated_at = self.updated_at.max(updated_at);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("task workspace", self.schema_version)?;
        validate_timestamp(self.updated_at)?;
        if self.tasks.len() > MAX_TASKS {
            return invalid("tasks", "exceeds the workspace limit");
        }
        let ids = self
            .tasks
            .iter()
            .map(|task| &task.task_id)
            .collect::<BTreeSet<_>>();
        if ids.len() != self.tasks.len() {
            return invalid("task_id", "contains duplicates");
        }
        for task in &self.tasks {
            task.validate()?;
            if task.updated_at > self.updated_at {
                return invalid("updated_at", "precedes a task update");
            }
            if let Some(parent_id) = task.parent_task_id.as_ref() {
                if !ids.contains(parent_id) {
                    return invalid("parent_task_id", "names a missing task");
                }
                self.depth(parent_id).map_err(|error| {
                    ProjectAgentValidationError::InvalidField {
                        field: "parent_task_id",
                        reason: error.to_string(),
                    }
                })?;
            }
        }
        Ok(())
    }

    fn task_mut(
        &mut self,
        task_id: &ProjectTaskId,
    ) -> Result<&mut ProjectTaskNode, ProjectTaskWorkspaceError> {
        self.tasks
            .iter_mut()
            .find(|task| task.task_id == *task_id)
            .ok_or_else(|| ProjectTaskWorkspaceError::TaskNotFound(task_id.clone()))
    }

    fn depth(&self, task_id: &ProjectTaskId) -> Result<usize, ProjectTaskWorkspaceError> {
        let mut depth = 1;
        let mut task = self
            .task(task_id)
            .ok_or_else(|| ProjectTaskWorkspaceError::TaskNotFound(task_id.clone()))?;
        let mut seen = BTreeSet::from([task_id]);
        while let Some(parent_id) = task.parent_task_id.as_ref() {
            if !seen.insert(parent_id) || depth == MAX_DEPTH {
                return Err(ProjectTaskWorkspaceError::Limit("task depth"));
            }
            task = self
                .task(parent_id)
                .ok_or_else(|| ProjectTaskWorkspaceError::TaskNotFound(parent_id.clone()))?;
            depth += 1;
        }
        Ok(depth)
    }
}

impl ProjectTaskNode {
    fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_task_id(self.task_id.as_str())?;
        validate_bounded_text(
            "title",
            &self.title,
            MAX_TITLE_BYTES,
            /*required*/ true,
        )?;
        validate_bounded_text(
            "objective",
            &self.objective,
            MAX_OBJECTIVE_BYTES,
            /*required*/ true,
        )?;
        validate_list("requirements", &self.requirements, MAX_REQUIREMENTS)?;
        validate_update(self, self.updated_at)?;
        if let Some(execution_task_id) = self.execution_task_id.as_deref() {
            validate_task_id(execution_task_id)?;
            if !matches!(&self.executor, ProjectTaskExecutor::ProjectAgent { .. }) {
                return invalid("execution_task_id", "requires a project AGENT executor");
            }
        }
        match (&self.result, self.status) {
            (None, ProjectTaskStatus::Pending | ProjectTaskStatus::InProgress) => {}
            (Some(result), status) if status == ProjectTaskStatus::from(result.status) => {
                result.validate()?
            }
            _ => return invalid("status", "does not match the task result"),
        }
        if let Some(result) = self.result.as_ref() {
            if result.completed_at < self.created_at || result.completed_at > self.updated_at {
                return invalid("completed_at", "falls outside the task lifetime");
            }
        }
        if let Some(evaluation) = self.evaluation.as_ref() {
            if self.result.is_none() {
                return invalid("evaluation", "requires a result");
            }
            evaluation.validate()?;
            if evaluation.evaluated_at < self.created_at
                || evaluation.evaluated_at > self.updated_at
            {
                return invalid("evaluated_at", "falls outside the task lifetime");
            }
        }
        Ok(())
    }
}

impl ProjectTaskResult {
    fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_bounded_text(
            "result",
            &self.summary,
            MAX_RESULT_BYTES,
            /*required*/ true,
        )?;
        validate_list("artifacts", &self.artifacts, MAX_RESULT_ITEMS)?;
        validate_list("evidence", &self.evidence, MAX_RESULT_ITEMS)?;
        if self.suggested_children.len() > MAX_SUGGESTIONS {
            return invalid("suggested_children", "exceeds the suggestion limit");
        }
        for child in &self.suggested_children {
            validate_bounded_text(
                "title",
                &child.title,
                MAX_TITLE_BYTES,
                /*required*/ true,
            )?;
            validate_bounded_text(
                "objective",
                &child.objective,
                MAX_OBJECTIVE_BYTES,
                /*required*/ true,
            )?;
        }
        match (self.status, self.error.as_deref()) {
            (ProjectTaskResultStatus::Failed, Some(error)) => {
                validate_bounded_text("error", error, MAX_ITEM_BYTES, /*required*/ true)?;
            }
            (ProjectTaskResultStatus::Failed, None) => {
                return invalid("error", "is required for a failed result");
            }
            (_, Some(_)) => return invalid("error", "is only valid for a failed result"),
            (_, None) => {}
        }
        validate_timestamp(self.completed_at)
    }
}

impl ProjectTaskEvaluation {
    fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_bounded_text(
            "evaluation",
            &self.summary,
            MAX_EVALUATION_BYTES,
            /*required*/ true,
        )?;
        validate_list("evaluation evidence", &self.evidence, MAX_RESULT_ITEMS)?;
        validate_timestamp(self.evaluated_at)
    }
}

impl From<ProjectTaskResultStatus> for ProjectTaskStatus {
    fn from(status: ProjectTaskResultStatus) -> Self {
        match status {
            ProjectTaskResultStatus::Completed => Self::Completed,
            ProjectTaskResultStatus::Rejected => Self::Rejected,
            ProjectTaskResultStatus::BlockedMissingTool => Self::Blocked,
            ProjectTaskResultStatus::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProjectTaskWorkspaceError {
    #[error(transparent)]
    Invalid(#[from] ProjectAgentValidationError),
    #[error("semantic task `{0}` already exists")]
    TaskAlreadyExists(ProjectTaskId),
    #[error("semantic task `{0}` was not found")]
    TaskNotFound(ProjectTaskId),
    #[error("semantic task workspace exceeded its {0} limit")]
    Limit(&'static str),
    #[error("cannot {operation} for semantic task `{task_id}` while it is {status:?}")]
    InvalidState {
        task_id: ProjectTaskId,
        status: ProjectTaskStatus,
        operation: &'static str,
    },
}

fn ensure_active(
    task: &ProjectTaskNode,
    operation: &'static str,
) -> Result<(), ProjectTaskWorkspaceError> {
    if matches!(
        task.status,
        ProjectTaskStatus::Pending | ProjectTaskStatus::InProgress
    ) {
        Ok(())
    } else {
        Err(invalid_state(task, operation))
    }
}

fn invalid_state(task: &ProjectTaskNode, operation: &'static str) -> ProjectTaskWorkspaceError {
    ProjectTaskWorkspaceError::InvalidState {
        task_id: task.task_id.clone(),
        status: task.status,
        operation,
    }
}

fn validate_update(
    task: &ProjectTaskNode,
    updated_at: i64,
) -> Result<(), ProjectAgentValidationError> {
    if updated_at < task.created_at || updated_at < task.updated_at {
        return invalid("updated_at", "must not move backwards");
    }
    validate_timestamp(updated_at)
}

fn validate_list(
    field: &'static str,
    values: &[String],
    maximum: usize,
) -> Result<(), ProjectAgentValidationError> {
    if values.len() > maximum {
        return invalid(field, "contains too many items");
    }
    for value in values {
        validate_bounded_text(field, value, MAX_ITEM_BYTES, /*required*/ true)?;
    }
    Ok(())
}

fn validate_timestamp(timestamp: i64) -> Result<(), ProjectAgentValidationError> {
    if timestamp < 0 {
        return invalid("timestamp", "must be non-negative");
    }
    Ok(())
}

fn invalid<T>(
    field: &'static str,
    reason: impl Into<String>,
) -> Result<T, ProjectAgentValidationError> {
    Err(ProjectAgentValidationError::InvalidField {
        field,
        reason: reason.into(),
    })
}
