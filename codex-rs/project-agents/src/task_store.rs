use crate::ProjectAgentFileSystemScope;
use crate::ProjectAgentId;
use crate::ProjectAgentSessionMetadata;
use crate::ProjectAgentStore;
use crate::ProjectAgentStoreError;
use crate::ProjectAgentTaskListLimit;
use crate::ProjectAgentTaskMetadata;
use crate::ProjectAgentTaskResult;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_utils_path_uri::PathUri;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io;

const TASK_METADATA_DIRECTORY: &str = "tasks/metadata";
const TASK_CURRENT_METADATA_PATH: &str = "tasks/current-task.json";
const TASK_HISTORY_DIRECTORY: &str = "tasks/history";
const TASK_CURRENT_RESULT_PATH: &str = "tasks/current.json";
const SESSION_METADATA_PATH: &str = "session.json";
const MAX_TASK_DIRECTORY_ENTRIES: usize = 1_024;
const MAX_TASK_METADATA_BYTES: usize = 32 * 1024;
const MAX_SESSION_METADATA_BYTES: usize = 8 * 1024;
const MAX_TASK_RESULT_BYTES: usize = 256 * 1024;

impl ProjectAgentStore {
    pub async fn persist_session_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        metadata: &ProjectAgentSessionMetadata,
    ) -> Result<PathUri, ProjectAgentStoreError> {
        metadata.validate()?;
        let path = self.resolve_agent_relative(&metadata.agent_id, SESSION_METADATA_PATH)?;
        self.write_json_document(
            file_system,
            scope,
            &path,
            metadata,
            "session metadata",
            MAX_SESSION_METADATA_BYTES,
        )
        .await?;
        Ok(path)
    }

    pub async fn session_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<Option<ProjectAgentSessionMetadata>, ProjectAgentStoreError> {
        let path = self.resolve_agent_relative(agent_id, SESSION_METADATA_PATH)?;
        let Some(metadata) = self
            .read_optional_json(file_system, scope, &path, MAX_SESSION_METADATA_BYTES)
            .await?
        else {
            return Ok(None);
        };
        let metadata: ProjectAgentSessionMetadata = metadata;
        metadata.validate()?;
        if metadata.agent_id != *agent_id {
            return Err(crate::ProjectAgentValidationError::InvalidField {
                field: "agent_id",
                reason: format!(
                    "session metadata names `{}`, expected `{agent_id}`",
                    metadata.agent_id
                ),
            }
            .into());
        }
        Ok(Some(metadata))
    }

    pub async fn persist_task_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        metadata: &ProjectAgentTaskMetadata,
    ) -> Result<(PathUri, PathUri), ProjectAgentStoreError> {
        metadata.validate_for(&metadata.agent_id, &metadata.task_id)?;
        let history_path = self.resolve_agent_relative(
            &metadata.agent_id,
            &format!("{TASK_METADATA_DIRECTORY}/{}.json", metadata.task_id),
        )?;
        let current_path =
            self.resolve_agent_relative(&metadata.agent_id, TASK_CURRENT_METADATA_PATH)?;
        for path in [&history_path, &current_path] {
            self.write_json_document(
                file_system,
                scope,
                path,
                metadata,
                "task metadata",
                MAX_TASK_METADATA_BYTES,
            )
            .await?;
        }
        Ok((current_path, history_path))
    }

    pub async fn current_task_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<Option<ProjectAgentTaskMetadata>, ProjectAgentStoreError> {
        let path = self.resolve_agent_relative(agent_id, TASK_CURRENT_METADATA_PATH)?;
        let Some(metadata) = self
            .read_optional_json(file_system, scope, &path, MAX_TASK_METADATA_BYTES)
            .await?
        else {
            return Ok(None);
        };
        let metadata: ProjectAgentTaskMetadata = metadata;
        metadata.validate_for(agent_id, &metadata.task_id)?;
        Ok(Some(metadata))
    }

    pub async fn task_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        task_id: &str,
    ) -> Result<Option<ProjectAgentTaskMetadata>, ProjectAgentStoreError> {
        let path = self.resolve_agent_relative(
            agent_id,
            &format!("{TASK_METADATA_DIRECTORY}/{task_id}.json"),
        )?;
        let Some(metadata) = self
            .read_optional_json(file_system, scope, &path, MAX_TASK_METADATA_BYTES)
            .await?
        else {
            return Ok(None);
        };
        let metadata: ProjectAgentTaskMetadata = metadata;
        metadata.validate_for(agent_id, task_id)?;
        Ok(Some(metadata))
    }

    pub async fn list_task_metadata(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        limit: ProjectAgentTaskListLimit,
    ) -> Result<Vec<ProjectAgentTaskMetadata>, ProjectAgentStoreError> {
        let names = self
            .task_json_names(file_system, scope, agent_id, TASK_METADATA_DIRECTORY)
            .await?;
        let mut tasks = Vec::with_capacity(names.len().min(limit.get()));
        for task_id in names.into_iter().take(limit.get()) {
            if let Some(metadata) = self
                .task_metadata(file_system, scope, agent_id, &task_id)
                .await?
            {
                tasks.push(metadata);
            }
        }
        Ok(tasks)
    }

    pub async fn current_task_result(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<Option<ProjectAgentTaskResult>, ProjectAgentStoreError> {
        let path = self.resolve_agent_relative(agent_id, TASK_CURRENT_RESULT_PATH)?;
        let Some(result) = self
            .read_optional_json(file_system, scope, &path, MAX_TASK_RESULT_BYTES)
            .await?
        else {
            return Ok(None);
        };
        let result: ProjectAgentTaskResult = result;
        result.validate_for(agent_id, &result.task_id)?;
        Ok(Some(result))
    }

    pub async fn task_result(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        task_id: &str,
    ) -> Result<Option<ProjectAgentTaskResult>, ProjectAgentStoreError> {
        let path = self.resolve_agent_relative(
            agent_id,
            &format!("{TASK_HISTORY_DIRECTORY}/{task_id}.json"),
        )?;
        let Some(result) = self
            .read_optional_json(file_system, scope, &path, MAX_TASK_RESULT_BYTES)
            .await?
        else {
            return Ok(None);
        };
        let result: ProjectAgentTaskResult = result;
        result.validate_for(agent_id, task_id)?;
        Ok(Some(result))
    }

    pub async fn list_task_results(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        limit: ProjectAgentTaskListLimit,
    ) -> Result<Vec<ProjectAgentTaskResult>, ProjectAgentStoreError> {
        let names = self
            .task_json_names(file_system, scope, agent_id, TASK_HISTORY_DIRECTORY)
            .await?;
        let mut results = Vec::with_capacity(names.len().min(limit.get()));
        for task_id in names.into_iter().take(limit.get()) {
            if let Some(result) = self
                .task_result(file_system, scope, agent_id, &task_id)
                .await?
            {
                results.push(result);
            }
        }
        Ok(results)
    }

    async fn task_json_names(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        directory_relative: &str,
    ) -> Result<Vec<String>, ProjectAgentStoreError> {
        let directory = self.resolve_agent_relative(agent_id, directory_relative)?;
        let mut entries = match file_system
            .read_directory(&directory, scope.sandbox())
            .await
        {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(self.file_system_error("read", &directory, source)),
        };
        if entries.len() > MAX_TASK_DIRECTORY_ENTRIES {
            return Err(ProjectAgentStoreError::TooManyTaskEntries {
                path: directory,
                maximum: MAX_TASK_DIRECTORY_ENTRIES,
            });
        }
        entries.sort_by(|left, right| right.file_name.cmp(&left.file_name));
        Ok(entries
            .into_iter()
            .filter(|entry| entry.is_file)
            .filter_map(|entry| entry.file_name.strip_suffix(".json").map(str::to_string))
            .collect())
    }

    pub(crate) async fn read_optional_json<T>(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        maximum: usize,
    ) -> Result<Option<T>, ProjectAgentStoreError>
    where
        T: DeserializeOwned,
    {
        let contents = match self
            .read_text_bounded(file_system, scope, path, maximum)
            .await
        {
            Ok(contents) => contents,
            Err(ProjectAgentStoreError::FileSystem { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        Ok(Some(serde_json::from_str(&contents)?))
    }

    pub(crate) async fn write_json_document<T>(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        value: &T,
        document: &'static str,
        maximum: usize,
    ) -> Result<(), ProjectAgentStoreError>
    where
        T: Serialize,
    {
        let serialized = serde_json::to_vec_pretty(value)?;
        if serialized.len() > maximum {
            return Err(ProjectAgentStoreError::PersistedDocumentTooLarge {
                document,
                actual: serialized.len(),
                maximum,
            });
        }
        let Some(parent) = path.parent() else {
            return Err(self.file_system_error(
                "resolve parent for",
                path,
                io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
            ));
        };
        file_system
            .create_directory(
                &parent,
                CreateDirectoryOptions { recursive: true },
                scope.sandbox(),
            )
            .await
            .map_err(|source| self.file_system_error("create", &parent, source))?;
        file_system
            .write_file(path, serialized, scope.sandbox())
            .await
            .map_err(|source| self.file_system_error("write", path, source))
    }
}
