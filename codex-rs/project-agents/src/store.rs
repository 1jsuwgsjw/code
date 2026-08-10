use crate::ProjectAgentRegistry;
use crate::ProjectAgentValidationError;
use crate::RelativeProjectAgentPath;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::FindUpErrorPolicy;
use codex_file_system::find_nearest_ancestor_with_markers;
use codex_utils_path_uri::PathUri;
use codex_utils_path_uri::PathUriParseError;
use std::io;
use thiserror::Error;

const AGENT_DIRECTORY: &str = "AGENT";
const REGISTRY_FILE: &str = "registry.toml";

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

/// Selects whether project AGENT filesystem operations use a sandbox context.
#[derive(Clone, Copy, Debug, Default)]
pub enum ProjectAgentFileSystemScope<'a> {
    #[default]
    Unrestricted,
    Sandboxed(&'a FileSystemSandboxContext),
}

impl<'a> ProjectAgentFileSystemScope<'a> {
    fn sandbox(self) -> Option<&'a FileSystemSandboxContext> {
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

    fn file_system_error(
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
    #[error("failed to serialize project AGENT registry: {0}")]
    SerializeRegistry(#[from] toml::ser::Error),
}
