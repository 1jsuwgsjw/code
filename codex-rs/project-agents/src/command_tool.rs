use crate::PROJECT_AGENT_SCHEMA_VERSION;
use crate::ProjectAgentEntry;
use crate::ProjectAgentFileSystemScope;
use crate::ProjectAgentId;
use crate::ProjectAgentStore;
use crate::ProjectAgentStoreError;
use crate::ProjectAgentToolManifest;
use crate::ProjectAgentToolTarget;
use crate::ProjectAgentValidationError;
use crate::RelativeProjectAgentPath;
use crate::store::MAX_INPUT_SCHEMA_BYTES;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use serde_json::Value;

/// Declares one command-backed callable interface for a project AGENT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectAgentCommandToolRegistration {
    pub agent_id: ProjectAgentId,
    pub tool_id: ProjectAgentId,
    pub description: String,
    pub program: RelativeProjectAgentPath,
    pub input_schema: RelativeProjectAgentPath,
    pub timeout_ms: Option<u64>,
}

impl ProjectAgentCommandToolRegistration {
    fn manifest_path(&self) -> Result<RelativeProjectAgentPath, ProjectAgentValidationError> {
        RelativeProjectAgentPath::new(format!("tools/{}.x", self.tool_id))
    }

    fn manifest(&self) -> Result<ProjectAgentToolManifest, ProjectAgentValidationError> {
        validate_agent_tool_path("program", &self.program, /*required_suffix*/ None)?;
        validate_agent_tool_path("input_schema", &self.input_schema, Some(".json"))?;
        let manifest = ProjectAgentToolManifest {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: self.tool_id.clone(),
            description: self.description.clone(),
            target: ProjectAgentToolTarget::Command {
                program: self.program.clone(),
            },
            timeout_ms: self.timeout_ms,
            input_schema: Some(self.input_schema.clone()),
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

impl ProjectAgentStore {
    pub async fn register_command_tool(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        registration: ProjectAgentCommandToolRegistration,
    ) -> Result<ProjectAgentEntry, ProjectAgentStoreError> {
        let manifest = registration.manifest()?;
        let manifest_path = registration.manifest_path()?;
        let mut entry = self.get(file_system, scope, &registration.agent_id).await?;
        let resolved_manifest =
            self.resolve_agent_relative(&registration.agent_id, manifest_path.as_str())?;
        if entry.definition.tools.contains(&manifest_path) {
            return Err(ProjectAgentStoreError::ArtifactAlreadyExists(
                resolved_manifest,
            ));
        }

        let resolved_program =
            self.resolve_agent_relative(&registration.agent_id, registration.program.as_str())?;
        require_regular_file(self, file_system, scope, "program", &resolved_program).await?;
        let resolved_schema = self
            .resolve_agent_relative(&registration.agent_id, registration.input_schema.as_str())?;
        require_regular_file(self, file_system, scope, "input_schema", &resolved_schema).await?;
        let schema_contents = self
            .read_text_bounded(file_system, scope, &resolved_schema, MAX_INPUT_SCHEMA_BYTES)
            .await?;
        let schema = serde_json::from_str(&schema_contents).map_err(|source| {
            ProjectAgentStoreError::ParseInputSchema {
                path: resolved_schema.clone(),
                source,
            }
        })?;
        validate_bounded_parameter_contract(&schema)?;

        let tools_directory = self.resolve_agent_relative(&registration.agent_id, "tools")?;
        file_system
            .create_directory(
                &tools_directory,
                CreateDirectoryOptions { recursive: true },
                scope.sandbox(),
            )
            .await
            .map_err(|source| self.file_system_error("create", &tools_directory, source))?;

        entry.definition.tools.push(manifest_path);
        entry.definition.validate()?;
        let definition_path = self.resolve(&entry.path)?;
        let definition_contents = toml::to_string_pretty(&entry.definition).map_err(|source| {
            ProjectAgentStoreError::SerializeDefinition {
                path: definition_path.clone(),
                source,
            }
        })?;
        let manifest_contents = toml::to_string_pretty(&manifest).map_err(|source| {
            ProjectAgentStoreError::SerializeToolManifest {
                path: resolved_manifest.clone(),
                source,
            }
        })?;

        self.write_new_file(
            file_system,
            scope,
            &resolved_manifest,
            manifest_contents.into_bytes(),
        )
        .await?;
        file_system
            .write_file(
                &definition_path,
                definition_contents.into_bytes(),
                scope.sandbox(),
            )
            .await
            .map_err(|source| self.file_system_error("write", &definition_path, source))?;
        Ok(entry)
    }
}

fn validate_agent_tool_path(
    field: &'static str,
    path: &RelativeProjectAgentPath,
    required_suffix: Option<&str>,
) -> Result<(), ProjectAgentValidationError> {
    if !path.as_str().starts_with("tools/") {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("`{path}` must be under tools/"),
        });
    }
    if let Some(required_suffix) = required_suffix
        && !path.as_str().ends_with(required_suffix)
    {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("`{path}` must end in {required_suffix}"),
        });
    }
    Ok(())
}

async fn require_regular_file(
    store: &ProjectAgentStore,
    file_system: &dyn ExecutorFileSystem,
    scope: ProjectAgentFileSystemScope<'_>,
    field: &'static str,
    path: &codex_utils_path_uri::PathUri,
) -> Result<(), ProjectAgentStoreError> {
    let metadata = file_system
        .get_metadata(path, scope.sandbox())
        .await
        .map_err(|source| store.file_system_error("inspect", path, source))?;
    if !metadata.is_file {
        return Err(ProjectAgentValidationError::InvalidField {
            field,
            reason: format!("`{path}` must identify a file"),
        }
        .into());
    }
    Ok(())
}

fn validate_bounded_parameter_contract(schema: &Value) -> Result<(), ProjectAgentValidationError> {
    let Some(schema) = schema.as_object() else {
        return Err(invalid_input_schema(
            "must be a JSON object containing an object parameter schema",
        ));
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(invalid_input_schema("type must be `object`"));
    }
    if let Some(properties) = schema.get("properties")
        && !properties.is_object()
    {
        return Err(invalid_input_schema("properties must be a JSON object"));
    }
    if let Some(required) = schema.get("required")
        && required
            .as_array()
            .is_none_or(|items| items.iter().any(|item| !item.is_string()))
    {
        return Err(invalid_input_schema(
            "required must be an array of property names",
        ));
    }
    if schema.get("additionalProperties") != Some(&Value::Bool(false)) {
        return Err(invalid_input_schema(
            "additionalProperties must be false for a bounded callable interface",
        ));
    }
    Ok(())
}

fn invalid_input_schema(reason: &str) -> ProjectAgentValidationError {
    ProjectAgentValidationError::InvalidField {
        field: "input_schema",
        reason: reason.to_string(),
    }
}
