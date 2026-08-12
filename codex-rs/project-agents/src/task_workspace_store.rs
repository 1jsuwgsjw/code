use crate::ProjectAgentFileSystemScope;
use crate::ProjectAgentStore;
use crate::ProjectAgentStoreError;
use crate::ProjectTaskWorkspace;
use crate::RelativeProjectAgentPath;
use codex_file_system::ExecutorFileSystem;
use codex_utils_path_uri::PathUri;

const TASK_WORKSPACE_PATH: &str = "tasks/workspace.json";
const MAX_TASK_WORKSPACE_BYTES: usize = 2 * 1024 * 1024;

impl ProjectAgentStore {
    pub async fn task_workspace(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
    ) -> Result<ProjectTaskWorkspace, ProjectAgentStoreError> {
        let path = self.resolve(&RelativeProjectAgentPath::new(TASK_WORKSPACE_PATH)?)?;
        let workspace: ProjectTaskWorkspace = self
            .read_optional_json(file_system, scope, &path, MAX_TASK_WORKSPACE_BYTES)
            .await?
            .unwrap_or_default();
        workspace.validate()?;
        Ok(workspace)
    }

    pub async fn persist_task_workspace(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        workspace: &ProjectTaskWorkspace,
    ) -> Result<PathUri, ProjectAgentStoreError> {
        workspace.validate()?;
        let path = self.resolve(&RelativeProjectAgentPath::new(TASK_WORKSPACE_PATH)?)?;
        self.write_json_document(
            file_system,
            scope,
            &path,
            workspace,
            "task workspace",
            MAX_TASK_WORKSPACE_BYTES,
        )
        .await?;
        Ok(path)
    }
}
