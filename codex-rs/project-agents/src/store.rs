use crate::PROJECT_AGENT_SCHEMA_VERSION;
use crate::ProjectAgentDefinition;
use crate::ProjectAgentId;
use crate::ProjectAgentMemoryIndex;
use crate::ProjectAgentRegistration;
use crate::ProjectAgentRegistry;
use crate::ProjectAgentTaskResult;
use crate::ProjectAgentToolManifest;
use crate::ProjectAgentValidationError;
use crate::RelativeProjectAgentPath;
use crate::maintenance::PendingProjectAgentItem;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::FindUpErrorPolicy;
use codex_file_system::find_nearest_ancestor_with_markers;
use codex_utils_path_uri::PathUri;
use codex_utils_path_uri::PathUriParseError;
use serde_json::Value;
use std::io;
use thiserror::Error;

const AGENT_DIRECTORY: &str = "AGENT";
const REGISTRY_FILE: &str = "registry.toml";
const DEFAULT_MEMORY_MAX_ITEMS: u32 = 32;
const DEFAULT_MEMORY_MAX_TOKENS: u32 = 4_000;
const MAX_CONSTRAINTS_BYTES: usize = 32 * 1024;
const MAX_TOOL_MANIFEST_BYTES: usize = 64 * 1024;
pub(crate) const MAX_INPUT_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_MEMORY_INDEX_BYTES: usize = 64 * 1024;
const MAX_MEMORY_ITEM_BYTES: usize = 32 * 1024;
const MAX_PERSISTED_RESULT_BYTES: usize = 256 * 1024;
const APPROX_BYTES_PER_TOKEN: usize = 4;
const DEFAULT_CONSTRAINTS: &str = "# Constraints\n\n- Work only within this AGENT's declared domain.\n- Return `rejected_out_of_scope` for work outside that domain.\n- Return `blocked_missing_tool` when a required registered tool is unavailable.\n";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapDisposition {
    Created,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapOutcome {
    pub disposition: BootstrapDisposition,
    pub registry: ProjectAgentRegistry,
}

/// A registry entry paired with its validated on-disk definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAgentEntry {
    pub path: RelativeProjectAgentPath,
    pub enabled: bool,
    pub definition: ProjectAgentDefinition,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedProjectAgentTool {
    pub manifest_path: RelativeProjectAgentPath,
    pub manifest: ProjectAgentToolManifest,
    pub input_schema: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectAgentRuntime {
    pub entry: ProjectAgentEntry,
    pub constraints: String,
    pub tools: Vec<LoadedProjectAgentTool>,
    pub accepted_memory: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAgentPersistenceOutcome {
    pub current_task_path: PathUri,
    pub history_path: PathUri,
    pub memory_candidate_paths: Vec<PathUri>,
    pub improvement_proposal_paths: Vec<PathUri>,
}

/// Selects whether project AGENT filesystem operations use a sandbox context.
#[derive(Clone, Copy, Debug, Default)]
pub enum ProjectAgentFileSystemScope<'a> {
    #[default]
    Unrestricted,
    Sandboxed(&'a FileSystemSandboxContext),
}

impl<'a> ProjectAgentFileSystemScope<'a> {
    pub(crate) fn sandbox(self) -> Option<&'a FileSystemSandboxContext> {
        match self {
            Self::Unrestricted => None,
            Self::Sandboxed(sandbox) => Some(sandbox),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAgentStore {
    project_root: PathUri,
    agent_root: PathUri,
    registry_path: PathUri,
}

impl ProjectAgentStore {
    pub fn new(project_root: PathUri) -> Result<Self, ProjectAgentStoreError> {
        let agent_root = project_root.join(AGENT_DIRECTORY)?;
        let registry_path = agent_root.join(REGISTRY_FILE)?;
        Ok(Self {
            project_root,
            agent_root,
            registry_path,
        })
    }

    pub fn project_root(&self) -> &PathUri {
        &self.project_root
    }

    pub fn agent_root(&self) -> &PathUri {
        &self.agent_root
    }

    pub fn registry_path(&self) -> &PathUri {
        &self.registry_path
    }

    pub fn resolve(
        &self,
        relative_path: &RelativeProjectAgentPath,
    ) -> Result<PathUri, ProjectAgentStoreError> {
        let resolved = self.agent_root.join(relative_path.as_str())?;
        if !resolved.starts_with(&self.agent_root) {
            return Err(ProjectAgentStoreError::PathEscapesAgentRoot(
                relative_path.clone(),
            ));
        }
        Ok(resolved)
    }

    pub async fn bootstrap(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
    ) -> Result<BootstrapOutcome, ProjectAgentStoreError> {
        match self.load(file_system, scope).await {
            Ok(registry) => {
                return Ok(BootstrapOutcome {
                    disposition: BootstrapDisposition::Existing,
                    registry,
                });
            }
            Err(ProjectAgentStoreError::FileSystem { source, .. })
                if source.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let sandbox = scope.sandbox();
        file_system
            .create_directory(
                &self.agent_root,
                CreateDirectoryOptions { recursive: true },
                sandbox,
            )
            .await
            .map_err(|source| self.file_system_error("create", &self.agent_root, source))?;

        let registry = ProjectAgentRegistry::default();
        let contents = toml::to_string_pretty(&registry)?;
        file_system
            .write_file(&self.registry_path, contents.into_bytes(), sandbox)
            .await
            .map_err(|source| self.file_system_error("write", &self.registry_path, source))?;
        Ok(BootstrapOutcome {
            disposition: BootstrapDisposition::Created,
            registry,
        })
    }

    pub async fn load(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
    ) -> Result<ProjectAgentRegistry, ProjectAgentStoreError> {
        let sandbox = scope.sandbox();
        let contents = file_system
            .read_file_text(&self.registry_path, sandbox)
            .await
            .map_err(|source| self.file_system_error("read", &self.registry_path, source))?;
        let registry: ProjectAgentRegistry =
            toml::from_str(&contents).map_err(|source| ProjectAgentStoreError::ParseRegistry {
                path: self.registry_path.clone(),
                source,
            })?;
        registry.validate()?;
        Ok(registry)
    }

    pub async fn list(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
    ) -> Result<Vec<ProjectAgentEntry>, ProjectAgentStoreError> {
        let registry = self.load(file_system, scope).await?;
        let mut entries = Vec::with_capacity(registry.agents.len());
        for (agent_id, registration) in &registry.agents {
            entries.push(
                self.load_entry(file_system, scope, agent_id, registration)
                    .await?,
            );
        }
        Ok(entries)
    }

    pub async fn get(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<ProjectAgentEntry, ProjectAgentStoreError> {
        let registry = self.load(file_system, scope).await?;
        let registration = registry
            .agents
            .get(agent_id)
            .ok_or_else(|| ProjectAgentStoreError::AgentNotFound(agent_id.clone()))?;
        self.load_entry(file_system, scope, agent_id, registration)
            .await
    }

    pub async fn create(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: ProjectAgentId,
        description: String,
    ) -> Result<ProjectAgentEntry, ProjectAgentStoreError> {
        let mut registry = self.bootstrap(file_system, scope).await?.registry;
        if registry.agents.contains_key(&agent_id) {
            return Err(ProjectAgentStoreError::AgentAlreadyExists(agent_id));
        }

        let agent_directory = RelativeProjectAgentPath::new(format!("agents/{agent_id}"))?;
        let agent_directory_path = self.resolve(&agent_directory)?;
        match file_system
            .get_metadata(&agent_directory_path, scope.sandbox())
            .await
        {
            Ok(_) => {
                return Err(ProjectAgentStoreError::AgentPathAlreadyExists(
                    agent_directory_path,
                ));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(self.file_system_error("inspect", &agent_directory_path, source));
            }
        }

        let definition_path =
            RelativeProjectAgentPath::new(format!("agents/{agent_id}/agent.toml"))?;
        let definition = ProjectAgentDefinition {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: agent_id.clone(),
            description,
            constraints_file: RelativeProjectAgentPath::new("constraints.md")?,
            model: None,
            model_reasoning_effort: None,
            model_provider: None,
            tools: Vec::new(),
            memory_max_items: DEFAULT_MEMORY_MAX_ITEMS,
            memory_max_tokens: DEFAULT_MEMORY_MAX_TOKENS,
        };
        definition.validate()?;

        let sandbox = scope.sandbox();
        file_system
            .create_directory(
                &agent_directory_path,
                CreateDirectoryOptions { recursive: true },
                sandbox,
            )
            .await
            .map_err(|source| self.file_system_error("create", &agent_directory_path, source))?;

        let constraints_path = self.resolve(&RelativeProjectAgentPath::new(format!(
            "agents/{agent_id}/constraints.md"
        ))?)?;
        file_system
            .write_file(
                &constraints_path,
                DEFAULT_CONSTRAINTS.as_bytes().to_vec(),
                sandbox,
            )
            .await
            .map_err(|source| self.file_system_error("write", &constraints_path, source))?;

        let resolved_definition_path = self.resolve(&definition_path)?;
        let definition_contents = toml::to_string_pretty(&definition).map_err(|source| {
            ProjectAgentStoreError::SerializeDefinition {
                path: resolved_definition_path.clone(),
                source,
            }
        })?;
        file_system
            .write_file(
                &resolved_definition_path,
                definition_contents.into_bytes(),
                sandbox,
            )
            .await
            .map_err(|source| self.file_system_error("write", &resolved_definition_path, source))?;

        registry.agents.insert(
            agent_id,
            ProjectAgentRegistration {
                path: definition_path.clone(),
                enabled: true,
            },
        );
        registry.revision = registry.revision.saturating_add(1);
        self.save_registry(file_system, scope, &registry).await?;

        Ok(ProjectAgentEntry {
            path: definition_path,
            enabled: true,
            definition,
        })
    }

    pub async fn disable(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<ProjectAgentEntry, ProjectAgentStoreError> {
        let mut registry = self.load(file_system, scope).await?;
        let registration = registry
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| ProjectAgentStoreError::AgentNotFound(agent_id.clone()))?;
        registration.enabled = false;
        let registration = registration.clone();
        registry.revision = registry.revision.saturating_add(1);
        self.save_registry(file_system, scope, &registry).await?;
        self.load_entry(file_system, scope, agent_id, &registration)
            .await
    }

    pub async fn load_runtime(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<ProjectAgentRuntime, ProjectAgentStoreError> {
        let entry = self.get(file_system, scope, agent_id).await?;
        if !entry.enabled {
            return Err(ProjectAgentStoreError::AgentDisabled(agent_id.clone()));
        }

        let constraints_path =
            self.resolve_agent_path(agent_id, &entry.definition.constraints_file)?;
        let constraints = self
            .read_text_bounded(file_system, scope, &constraints_path, MAX_CONSTRAINTS_BYTES)
            .await?;
        let mut tools = Vec::with_capacity(entry.definition.tools.len());
        for manifest_path in &entry.definition.tools {
            let resolved_path = self.resolve_agent_path(agent_id, manifest_path)?;
            let contents = self
                .read_text_bounded(file_system, scope, &resolved_path, MAX_TOOL_MANIFEST_BYTES)
                .await?;
            let manifest: ProjectAgentToolManifest =
                toml::from_str(&contents).map_err(|source| {
                    ProjectAgentStoreError::ParseToolManifest {
                        path: resolved_path.clone(),
                        source,
                    }
                })?;
            manifest.validate()?;
            let input_schema = match &manifest.input_schema {
                Some(input_schema_path) => {
                    let resolved_schema = self.resolve_agent_path(agent_id, input_schema_path)?;
                    let contents = self
                        .read_text_bounded(
                            file_system,
                            scope,
                            &resolved_schema,
                            MAX_INPUT_SCHEMA_BYTES,
                        )
                        .await?;
                    Some(serde_json::from_str(&contents).map_err(|source| {
                        ProjectAgentStoreError::ParseInputSchema {
                            path: resolved_schema,
                            source,
                        }
                    })?)
                }
                None => None,
            };
            tools.push(LoadedProjectAgentTool {
                manifest_path: manifest_path.clone(),
                manifest,
                input_schema,
            });
        }

        let accepted_memory = self
            .load_accepted_memory(file_system, scope, agent_id, &entry.definition)
            .await?;
        Ok(ProjectAgentRuntime {
            entry,
            constraints,
            tools,
            accepted_memory,
        })
    }

    pub async fn persist_result(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        result: &ProjectAgentTaskResult,
    ) -> Result<ProjectAgentPersistenceOutcome, ProjectAgentStoreError> {
        result.validate_for(&result.agent_id, &result.task_id)?;
        let serialized = serde_json::to_vec_pretty(result)?;
        if serialized.len() > MAX_PERSISTED_RESULT_BYTES {
            return Err(ProjectAgentStoreError::PersistedResultTooLarge {
                actual: serialized.len(),
                maximum: MAX_PERSISTED_RESULT_BYTES,
            });
        }

        let agent_id = &result.agent_id;
        let current_task_path = self.resolve_agent_relative(agent_id, "tasks/current.json")?;
        let history_directory = self.resolve_agent_relative(agent_id, "tasks/history")?;
        let history_path = self
            .resolve_agent_relative(agent_id, &format!("tasks/history/{}.json", result.task_id))?;
        let candidate_directory = self.resolve_agent_relative(agent_id, "memory/candidates")?;
        let proposal_directory = self.resolve_agent_relative(agent_id, "proposals/pending")?;
        let sandbox = scope.sandbox();
        for directory in [
            &history_directory,
            &candidate_directory,
            &proposal_directory,
        ] {
            file_system
                .create_directory(
                    directory,
                    CreateDirectoryOptions { recursive: true },
                    sandbox,
                )
                .await
                .map_err(|source| self.file_system_error("create", directory, source))?;
        }

        self.write_new_file(file_system, scope, &history_path, serialized.clone())
            .await?;
        file_system
            .write_file(&current_task_path, serialized, sandbox)
            .await
            .map_err(|source| self.file_system_error("write", &current_task_path, source))?;

        let mut memory_candidate_paths = Vec::with_capacity(result.memory_candidates.len());
        for (index, body) in result.memory_candidates.iter().enumerate() {
            let path = self.resolve_agent_relative(
                agent_id,
                &format!("memory/candidates/{}-{index}.json", result.task_id),
            )?;
            let item = PendingProjectAgentItem::new(
                agent_id.clone(),
                result.task_id.clone(),
                body.clone(),
            );
            self.write_new_file(file_system, scope, &path, serde_json::to_vec_pretty(&item)?)
                .await?;
            memory_candidate_paths.push(path);
        }

        let mut improvement_proposal_paths = Vec::with_capacity(result.improvement_proposals.len());
        for (index, body) in result.improvement_proposals.iter().enumerate() {
            let path = self.resolve_agent_relative(
                agent_id,
                &format!("proposals/pending/{}-{index}.json", result.task_id),
            )?;
            let item = PendingProjectAgentItem::new(
                agent_id.clone(),
                result.task_id.clone(),
                body.clone(),
            );
            self.write_new_file(file_system, scope, &path, serde_json::to_vec_pretty(&item)?)
                .await?;
            improvement_proposal_paths.push(path);
        }

        Ok(ProjectAgentPersistenceOutcome {
            current_task_path,
            history_path,
            memory_candidate_paths,
            improvement_proposal_paths,
        })
    }

    async fn load_accepted_memory(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        definition: &ProjectAgentDefinition,
    ) -> Result<Vec<String>, ProjectAgentStoreError> {
        let index_path = self.resolve_agent_relative(agent_id, "memory/index.toml")?;
        let index_contents = match self
            .read_text_bounded(file_system, scope, &index_path, MAX_MEMORY_INDEX_BYTES)
            .await
        {
            Ok(contents) => contents,
            Err(ProjectAgentStoreError::FileSystem { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let index: ProjectAgentMemoryIndex = toml::from_str(&index_contents).map_err(|source| {
            ProjectAgentStoreError::ParseMemoryIndex {
                path: index_path,
                source,
            }
        })?;
        index.validate()?;

        let max_items = usize::try_from(definition.memory_max_items).unwrap_or(usize::MAX);
        let max_tokens = usize::try_from(definition.memory_max_tokens).unwrap_or(usize::MAX);
        let mut accepted = Vec::new();
        let mut used_tokens = 0usize;
        for item in index.items.iter().take(max_items) {
            let path = self.resolve_agent_path(agent_id, item)?;
            let contents = self
                .read_text_bounded(file_system, scope, &path, MAX_MEMORY_ITEM_BYTES)
                .await?;
            let tokens = contents.len().div_ceil(APPROX_BYTES_PER_TOKEN);
            if used_tokens.saturating_add(tokens) > max_tokens {
                break;
            }
            used_tokens += tokens;
            accepted.push(contents);
        }
        Ok(accepted)
    }

    fn resolve_agent_path(
        &self,
        agent_id: &ProjectAgentId,
        path: &RelativeProjectAgentPath,
    ) -> Result<PathUri, ProjectAgentStoreError> {
        self.resolve_agent_relative(agent_id, path.as_str())
    }

    pub(crate) fn resolve_agent_relative(
        &self,
        agent_id: &ProjectAgentId,
        path: &str,
    ) -> Result<PathUri, ProjectAgentStoreError> {
        self.resolve(&RelativeProjectAgentPath::new(format!(
            "agents/{agent_id}/{path}"
        ))?)
    }

    pub(crate) async fn read_text_bounded(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        maximum: usize,
    ) -> Result<String, ProjectAgentStoreError> {
        let metadata = file_system
            .get_metadata(path, scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("inspect", path, source))?;
        if metadata.size > maximum as u64 {
            return Err(ProjectAgentStoreError::FileTooLarge {
                path: path.clone(),
                actual: metadata.size,
                maximum,
            });
        }
        let contents = file_system
            .read_file(path, scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("read", path, source))?;
        if contents.len() > maximum {
            return Err(ProjectAgentStoreError::FileTooLarge {
                path: path.clone(),
                actual: contents.len() as u64,
                maximum,
            });
        }
        String::from_utf8(contents).map_err(|source| {
            self.file_system_error(
                "decode",
                path,
                io::Error::new(io::ErrorKind::InvalidData, source),
            )
        })
    }

    pub(crate) async fn write_new_file(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        contents: Vec<u8>,
    ) -> Result<(), ProjectAgentStoreError> {
        match file_system.get_metadata(path, scope.sandbox()).await {
            Ok(_) => return Err(ProjectAgentStoreError::ArtifactAlreadyExists(path.clone())),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(self.file_system_error("inspect", path, source)),
        }
        file_system
            .write_file(path, contents, scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("write", path, source))
    }

    async fn save_registry(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        registry: &ProjectAgentRegistry,
    ) -> Result<(), ProjectAgentStoreError> {
        registry.validate()?;
        let contents = toml::to_string_pretty(registry)?;
        file_system
            .write_file(&self.registry_path, contents.into_bytes(), scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("write", &self.registry_path, source))
    }

    async fn load_entry(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        registration: &ProjectAgentRegistration,
    ) -> Result<ProjectAgentEntry, ProjectAgentStoreError> {
        let path = self.resolve(&registration.path)?;
        let contents = file_system
            .read_file_text(&path, scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("read", &path, source))?;
        let definition: ProjectAgentDefinition = toml::from_str(&contents).map_err(|source| {
            ProjectAgentStoreError::ParseDefinition {
                path: path.clone(),
                source,
            }
        })?;
        definition.validate()?;
        if definition.id != *agent_id {
            return Err(ProjectAgentStoreError::DefinitionIdMismatch {
                path,
                expected: agent_id.clone(),
                actual: definition.id,
            });
        }
        Ok(ProjectAgentEntry {
            path: registration.path.clone(),
            enabled: registration.enabled,
            definition,
        })
    }

    pub(crate) fn file_system_error(
        &self,
        operation: &'static str,
        path: &PathUri,
        source: io::Error,
    ) -> ProjectAgentStoreError {
        ProjectAgentStoreError::FileSystem {
            operation,
            path: path.clone(),
            source,
        }
    }
}

pub async fn resolve_project_root(
    file_system: &dyn ExecutorFileSystem,
    cwd: &PathUri,
    markers: &[RelativeProjectAgentPath],
    scope: ProjectAgentFileSystemScope<'_>,
) -> Result<PathUri, ProjectAgentStoreError> {
    if markers.is_empty() {
        return Ok(cwd.clone());
    }
    let markers = markers
        .iter()
        .map(|marker| marker.as_str().to_string())
        .collect();
    find_nearest_ancestor_with_markers(
        file_system,
        cwd,
        markers,
        FindUpErrorPolicy::Propagate,
        scope.sandbox(),
    )
    .await
    .map(|root| root.unwrap_or_else(|| cwd.clone()))
    .map_err(|source| ProjectAgentStoreError::FileSystem {
        operation: "resolve project root from",
        path: cwd.clone(),
        source,
    })
}

#[derive(Debug, Error)]
pub enum ProjectAgentStoreError {
    #[error(transparent)]
    InvalidPath(#[from] PathUriParseError),
    #[error(transparent)]
    InvalidDefinition(#[from] ProjectAgentValidationError),
    #[error("project AGENT path `{0}` escapes the AGENT root")]
    PathEscapesAgentRoot(RelativeProjectAgentPath),
    #[error("project AGENT `{0}` is already registered")]
    AgentAlreadyExists(ProjectAgentId),
    #[error("project AGENT `{0}` is not registered")]
    AgentNotFound(ProjectAgentId),
    #[error("project AGENT `{0}` is disabled")]
    AgentDisabled(ProjectAgentId),
    #[error("project AGENT path `{0}` already exists but is not registered")]
    AgentPathAlreadyExists(PathUri),
    #[error(
        "project AGENT definition `{path}` has id `{actual}`, but the registry expects `{expected}`"
    )]
    DefinitionIdMismatch {
        path: PathUri,
        expected: ProjectAgentId,
        actual: ProjectAgentId,
    },
    #[error("failed to {operation} `{path}`: {source}")]
    FileSystem {
        operation: &'static str,
        path: PathUri,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse project AGENT registry `{path}`: {source}")]
    ParseRegistry {
        path: PathUri,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to parse project AGENT definition `{path}`: {source}")]
    ParseDefinition {
        path: PathUri,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to parse project AGENT tool manifest `{path}`: {source}")]
    ParseToolManifest {
        path: PathUri,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to parse project AGENT memory index `{path}`: {source}")]
    ParseMemoryIndex {
        path: PathUri,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to parse project AGENT input schema `{path}`: {source}")]
    ParseInputSchema {
        path: PathUri,
        #[source]
        source: serde_json::Error,
    },
    #[error("project AGENT file `{path}` is {actual} bytes; maximum is {maximum}")]
    FileTooLarge {
        path: PathUri,
        actual: u64,
        maximum: usize,
    },
    #[error("project AGENT artifact `{0}` already exists")]
    ArtifactAlreadyExists(PathUri),
    #[error("serialized project AGENT result is {actual} bytes; maximum is {maximum}")]
    PersistedResultTooLarge { actual: usize, maximum: usize },
    #[error("serialized project AGENT {document} is {actual} bytes; maximum is {maximum}")]
    PersistedDocumentTooLarge {
        document: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("project AGENT task directory `{path}` exceeds {maximum} entries")]
    TooManyTaskEntries { path: PathUri, maximum: usize },
    #[error("failed to serialize project AGENT JSON: {0}")]
    SerializeJson(#[from] serde_json::Error),
    #[error("failed to serialize project AGENT registry: {0}")]
    SerializeRegistry(#[from] toml::ser::Error),
    #[error("failed to serialize project AGENT definition `{path}`: {source}")]
    SerializeDefinition {
        path: PathUri,
        #[source]
        source: toml::ser::Error,
    },
    #[error("failed to serialize project AGENT tool manifest `{path}`: {source}")]
    SerializeToolManifest {
        path: PathUri,
        #[source]
        source: toml::ser::Error,
    },
}
