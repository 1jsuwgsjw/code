use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::Error as _;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

pub const PROJECT_AGENT_SCHEMA_VERSION: u32 = 1;
const MAX_ID_BYTES: usize = 64;
const MAX_RELATIVE_PATH_BYTES: usize = 512;
const MAX_TASK_ID_BYTES: usize = 128;
const MAX_RESULT_TEXT_BYTES: usize = 64 * 1024;
const MAX_RESULT_LIST_ITEMS: usize = 128;
const MAX_RESULT_LIST_ITEM_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectAgentId(String);

impl ProjectAgentId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectAgentValidationError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_ID_BYTES
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
            })
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric);
        if !valid {
            return Err(ProjectAgentValidationError::InvalidIdentifier(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectAgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for ProjectAgentId {
    type Err = ProjectAgentValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ProjectAgentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RelativeProjectAgentPath(String);

impl RelativeProjectAgentPath {
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectAgentValidationError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_RELATIVE_PATH_BYTES
            && !value.starts_with('/')
            && !value.contains(['\\', ':', '\0'])
            && value
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        if !valid {
            return Err(ProjectAgentValidationError::InvalidRelativePath(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelativeProjectAgentPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RelativeProjectAgentPath {
    type Err = ProjectAgentValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for RelativeProjectAgentPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentRegistry {
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    pub agents: BTreeMap<ProjectAgentId, ProjectAgentRegistration>,
}

impl Default for ProjectAgentRegistry {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            revision: 0,
            agents: BTreeMap::new(),
        }
    }
}

impl ProjectAgentRegistry {
    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("registry.toml", self.schema_version)?;
        for (agent_id, registration) in &self.agents {
            let expected = format!("agents/{agent_id}/agent.toml");
            if registration.path.as_str() != expected {
                return Err(ProjectAgentValidationError::RegistryPathMismatch {
                    agent_id: agent_id.clone(),
                    expected,
                    actual: registration.path.to_string(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentRegistration {
    pub path: RelativeProjectAgentPath,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentDefinition {
    pub schema_version: u32,
    pub id: ProjectAgentId,
    pub description: String,
    pub constraints_file: RelativeProjectAgentPath,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub model_provider: Option<String>,
    pub tools: Vec<RelativeProjectAgentPath>,
    pub memory_max_items: u32,
    pub memory_max_tokens: u32,
}

impl ProjectAgentDefinition {
    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("agent.toml", self.schema_version)?;
        validate_non_empty("description", &self.description)?;
        for (field, value) in [
            ("model", &self.model),
            ("model_reasoning_effort", &self.model_reasoning_effort),
            ("model_provider", &self.model_provider),
        ] {
            if let Some(value) = value {
                validate_non_empty(field, value)?;
            }
        }
        let mut unique_tools = BTreeSet::new();
        for tool in &self.tools {
            if !tool.as_str().starts_with("tools/") || !tool.as_str().ends_with(".x") {
                return Err(ProjectAgentValidationError::InvalidField {
                    field: "tools",
                    reason: format!("tool manifest `{tool}` must be under tools/ and end in .x"),
                });
            }
            if !unique_tools.insert(tool) {
                return Err(ProjectAgentValidationError::InvalidField {
                    field: "tools",
                    reason: format!("duplicate tool manifest `{tool}`"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentToolManifest {
    pub schema_version: u32,
    pub id: ProjectAgentId,
    pub description: String,
    #[serde(flatten)]
    pub target: ProjectAgentToolTarget,
    pub timeout_ms: Option<u64>,
    pub input_schema: Option<RelativeProjectAgentPath>,
}

impl ProjectAgentToolManifest {
    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("tool manifest", self.schema_version)?;
        validate_non_empty("description", &self.description)?;
        if self.timeout_ms == Some(0) {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "timeout_ms",
                reason: "must be greater than zero when present".to_string(),
            });
        }
        match &self.target {
            ProjectAgentToolTarget::Native { tool } => validate_non_empty("tool", tool),
            ProjectAgentToolTarget::Command { .. } => Ok(()),
            ProjectAgentToolTarget::Mcp { server, tool } => {
                validate_non_empty("server", server)?;
                validate_non_empty("tool", tool)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentMemoryIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<RelativeProjectAgentPath>,
}

impl Default for ProjectAgentMemoryIndex {
    fn default() -> Self {
        Self {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            items: Vec::new(),
        }
    }
}

impl ProjectAgentMemoryIndex {
    pub fn validate(&self) -> Result<(), ProjectAgentValidationError> {
        validate_schema("memory index", self.schema_version)?;
        let mut unique = BTreeSet::new();
        for item in &self.items {
            let path = item.as_str();
            let valid_prefix = ["memory/facts/", "memory/experience/", "memory/bottlenecks/"]
                .iter()
                .any(|prefix| path.starts_with(prefix));
            if !valid_prefix || !unique.insert(item) {
                return Err(ProjectAgentValidationError::InvalidField {
                    field: "memory.items",
                    reason: format!("invalid or duplicate accepted-memory path `{item}`"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectAgentToolTarget {
    Native { tool: String },
    Command { program: RelativeProjectAgentPath },
    Mcp { server: String, tool: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAgentTaskStatus {
    Completed,
    RejectedOutOfScope,
    BlockedMissingTool,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectAgentTaskResult {
    pub status: ProjectAgentTaskStatus,
    pub agent_id: ProjectAgentId,
    pub task_id: String,
    pub result: String,
    pub artifacts: Vec<String>,
    pub evidence: Vec<String>,
    pub memory_candidates: Vec<String>,
    pub improvement_proposals: Vec<String>,
    #[serde(deserialize_with = "deserialize_required_optional_string")]
    pub error: Option<String>,
}

impl ProjectAgentTaskResult {
    pub fn validate_for(
        &self,
        expected_agent_id: &ProjectAgentId,
        expected_task_id: &str,
    ) -> Result<(), ProjectAgentValidationError> {
        if &self.agent_id != expected_agent_id {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "agent_id",
                reason: format!(
                    "result names `{}`, expected `{expected_agent_id}`",
                    self.agent_id
                ),
            });
        }
        if self.task_id != expected_task_id {
            return Err(ProjectAgentValidationError::InvalidField {
                field: "task_id",
                reason: format!(
                    "result names `{}`, expected `{expected_task_id}`",
                    self.task_id
                ),
            });
        }
        validate_task_id(&self.task_id)?;
        validate_bounded_text(
            "result",
            &self.result,
            MAX_RESULT_TEXT_BYTES,
            /*required*/ true,
        )?;
        validate_result_list("artifacts", &self.artifacts)?;
        validate_result_list("evidence", &self.evidence)?;
        validate_result_list("memory_candidates", &self.memory_candidates)?;
        validate_result_list("improvement_proposals", &self.improvement_proposals)?;
        match self.status {
            ProjectAgentTaskStatus::Failed => {
                let error = self.error.as_deref().ok_or_else(|| {
                    ProjectAgentValidationError::InvalidField {
                        field: "error",
                        reason: "is required when status is failed".to_string(),
                    }
                })?;
                validate_bounded_text(
                    "error",
                    error,
                    MAX_RESULT_LIST_ITEM_BYTES,
                    /*required*/ true,
                )
            }
            ProjectAgentTaskStatus::Completed
            | ProjectAgentTaskStatus::RejectedOutOfScope
            | ProjectAgentTaskStatus::BlockedMissingTool => {
                if self.error.is_some() {
                    return Err(ProjectAgentValidationError::InvalidField {
                        field: "error",
                        reason: "must be null unless status is failed".to_string(),
                    });
                }
                Ok(())
            }
        }
    }
}

fn deserialize_required_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProjectAgentValidationError {
    #[error("invalid project AGENT identifier `{0}`")]
    InvalidIdentifier(String),
    #[error("invalid project AGENT relative path `{0}`")]
    InvalidRelativePath(String),
    #[error("unsupported {document} schema version {found}; expected {expected}")]
    UnsupportedSchema {
        document: &'static str,
        found: u32,
        expected: u32,
    },
    #[error("invalid {field}: {reason}")]
    InvalidField { field: &'static str, reason: String },
    #[error("registry path for `{agent_id}` is `{actual}`; expected `{expected}`")]
    RegistryPathMismatch {
        agent_id: ProjectAgentId,
        expected: String,
        actual: String,
    },
}

pub(crate) fn validate_schema(
    document: &'static str,
    schema_version: u32,
) -> Result<(), ProjectAgentValidationError> {
    if schema_version != PROJECT_AGENT_SCHEMA_VERSION {
        return Err(ProjectAgentValidationError::UnsupportedSchema {
            document,
            found: schema_version,
            expected: PROJECT_AGENT_SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ProjectAgentValidationError> {
    if value.trim().is_empty() {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: "must not be empty".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn validate_task_id(value: &str) -> Result<(), ProjectAgentValidationError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_TASK_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte));
    if !valid {
        return Err(ProjectAgentValidationError::InvalidField {
            field: "task_id",
            reason: "must be a bounded ASCII identifier".to_string(),
        });
    }
    Ok(())
}

fn validate_result_list(
    field: &'static str,
    values: &[String],
) -> Result<(), ProjectAgentValidationError> {
    if values.len() > MAX_RESULT_LIST_ITEMS {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("contains more than {MAX_RESULT_LIST_ITEMS} items"),
        });
    }
    for value in values {
        validate_bounded_text(
            field,
            value,
            MAX_RESULT_LIST_ITEM_BYTES,
            /*required*/ true,
        )?;
    }
    Ok(())
}

pub(crate) fn validate_bounded_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
    required: bool,
) -> Result<(), ProjectAgentValidationError> {
    if (required && value.trim().is_empty()) || value.len() > max_bytes {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("must be non-empty and at most {max_bytes} bytes"),
        });
    }
    Ok(())
}
