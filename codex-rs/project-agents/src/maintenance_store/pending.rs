use super::*;

impl ProjectAgentStore {
    pub(super) async fn pending_counts(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<ProjectAgentPendingCounts, ProjectAgentMaintenanceError> {
        Ok(ProjectAgentPendingCounts {
            memory_candidates: self
                .pending_item_paths(
                    file_system,
                    scope,
                    agent_id,
                    ProjectAgentMaintenanceItemKind::MemoryCandidate,
                )
                .await?
                .len() as u64,
            improvement_proposals: self
                .pending_item_paths(
                    file_system,
                    scope,
                    agent_id,
                    ProjectAgentMaintenanceItemKind::ImprovementProposal,
                )
                .await?
                .len() as u64,
        })
    }

    pub(super) async fn pending_items(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        kind: ProjectAgentMaintenanceItemKind,
    ) -> Result<Vec<PendingItemFile>, ProjectAgentMaintenanceError> {
        let paths = self
            .pending_item_paths(file_system, scope, agent_id, kind)
            .await?;
        let mut items = Vec::with_capacity(paths.len());
        for (item_id, source_path) in paths {
            let path = self.resolve_agent_path(agent_id, &source_path)?;
            let item: PendingProjectAgentItem = self
                .read_json_bounded(file_system, scope, &path, MAX_PENDING_ITEM_BYTES)
                .await?;
            if item.schema_version != PROJECT_AGENT_SCHEMA_VERSION {
                return Err(ProjectAgentMaintenanceError::InvalidPendingItem {
                    path,
                    reason: format!(
                        "schema version {} does not match {}",
                        item.schema_version, PROJECT_AGENT_SCHEMA_VERSION
                    ),
                });
            }
            if item.agent_id != *agent_id || item.body.trim().is_empty() {
                return Err(ProjectAgentMaintenanceError::InvalidPendingItem {
                    path,
                    reason: "agent id does not match or body is empty".to_string(),
                });
            }
            items.push(PendingItemFile {
                item_id,
                source_path,
                item,
            });
        }
        Ok(items)
    }

    pub(super) async fn pending_item_paths(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        kind: ProjectAgentMaintenanceItemKind,
    ) -> Result<Vec<(String, RelativeProjectAgentPath)>, ProjectAgentMaintenanceError> {
        let directory_relative = match kind {
            ProjectAgentMaintenanceItemKind::MemoryCandidate => "memory/candidates",
            ProjectAgentMaintenanceItemKind::ImprovementProposal => "proposals/pending",
        };
        let directory = self.resolve_agent_relative(agent_id, directory_relative)?.1;
        let mut entries = match file_system.read_directory(&directory, sandbox(scope)).await {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(self.fs_error("read", &directory, source)),
        };
        if entries.len() > MAX_PENDING_DIRECTORY_ENTRIES {
            return Err(ProjectAgentMaintenanceError::TooManyDirectoryEntries {
                path: directory,
                maximum: MAX_PENDING_DIRECTORY_ENTRIES,
            });
        }
        entries.sort_by(|left, right| left.file_name.cmp(&right.file_name));
        let mut pending = Vec::new();
        for entry in entries {
            if !entry.is_file || !entry.file_name.ends_with(".json") {
                continue;
            }
            let source_path =
                RelativeProjectAgentPath::new(format!("{directory_relative}/{}", entry.file_name))?;
            let item_id = stable_item_id(kind, &source_path);
            if !self
                .decision_exists(file_system, scope, agent_id, kind, &item_id)
                .await?
            {
                pending.push((item_id, source_path));
            }
            if pending.len() > MAX_PENDING_ITEMS_PER_KIND {
                return Err(ProjectAgentMaintenanceError::TooManyPendingItems {
                    agent_id: agent_id.clone(),
                    kind,
                    maximum: MAX_PENDING_ITEMS_PER_KIND,
                });
            }
        }
        Ok(pending)
    }

    pub(super) async fn decision_exists(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        kind: ProjectAgentMaintenanceItemKind,
        item_id: &str,
    ) -> Result<bool, ProjectAgentMaintenanceError> {
        let paths = match kind {
            ProjectAgentMaintenanceItemKind::MemoryCandidate => {
                vec![RelativeProjectAgentPath::new(format!(
                    "memory/decisions/{item_id}.json"
                ))?]
            }
            ProjectAgentMaintenanceItemKind::ImprovementProposal => vec![
                RelativeProjectAgentPath::new(format!("proposals/accepted/{item_id}.json"))?,
                RelativeProjectAgentPath::new(format!("proposals/rejected/{item_id}.json"))?,
            ],
        };
        for path in paths {
            let path = self.resolve_agent_path(agent_id, &path)?;
            match file_system.get_metadata(&path, sandbox(scope)).await {
                Ok(_) => return Ok(true),
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(self.fs_error("inspect", &path, source)),
            }
        }
        Ok(false)
    }

    pub(super) async fn source_evidence(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        pending: &PendingItemFile,
        kind: ProjectAgentMaintenanceItemKind,
    ) -> Result<(Vec<String>, Option<String>), ProjectAgentMaintenanceError> {
        let task_path = match RelativeProjectAgentPath::new(format!(
            "tasks/history/{}.json",
            pending.item.task_id
        )) {
            Ok(path) => path,
            Err(error) => return Ok((Vec::new(), Some(error.to_string()))),
        };
        let task_path = self.resolve_agent_path(agent_id, &task_path)?;
        let task: ProjectAgentTaskResult = match self
            .read_json_bounded(file_system, scope, &task_path, MAX_TASK_RESULT_BYTES)
            .await
        {
            Ok(task) => task,
            Err(ProjectAgentMaintenanceError::FileSystem { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok((
                    Vec::new(),
                    Some("source task history is missing".to_string()),
                ));
            }
            Err(ProjectAgentMaintenanceError::ParseJson { source, .. }) => {
                return Ok((
                    Vec::new(),
                    Some(format!("source task history is invalid: {source}")),
                ));
            }
            Err(error) => return Err(error),
        };
        if let Err(error) = task.validate_for(agent_id, &pending.item.task_id) {
            return Ok((Vec::new(), Some(error.to_string())));
        }
        if task.status != ProjectAgentTaskStatus::Completed {
            return Ok((
                task.evidence,
                Some("source task did not complete successfully".to_string()),
            ));
        }
        if task.evidence.is_empty() {
            return Ok((
                Vec::new(),
                Some("source task contains no evidence".to_string()),
            ));
        }
        let listed = match kind {
            ProjectAgentMaintenanceItemKind::MemoryCandidate => task.memory_candidates,
            ProjectAgentMaintenanceItemKind::ImprovementProposal => task.improvement_proposals,
        };
        if !listed.iter().any(|body| body == &pending.item.body) {
            return Ok((
                task.evidence,
                Some("pending item is not attributable to its source task".to_string()),
            ));
        }
        Ok((task.evidence, None))
    }
}
