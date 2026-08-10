use super::*;

impl ProjectAgentStore {
    pub(super) async fn acquire_maintenance_lock(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        actor: &str,
        maintenance_id: &str,
        started_at_ms: i64,
        lock_directory: &PathUri,
    ) -> Result<(), ProjectAgentMaintenanceError> {
        let maintenance_directory = self
            .maintenance_resolve_agent_relative(agent_id, "maintenance")?
            .1;
        file_system
            .create_directory(
                &maintenance_directory,
                CreateDirectoryOptions { recursive: true },
                sandbox(scope),
            )
            .await
            .map_err(|source| self.fs_error("create", &maintenance_directory, source))?;
        match file_system
            .create_directory(
                lock_directory,
                CreateDirectoryOptions { recursive: false },
                sandbox(scope),
            )
            .await
        {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                return Err(ProjectAgentMaintenanceError::Locked {
                    agent_id: agent_id.clone(),
                    path: lock_directory.clone(),
                });
            }
            Err(source) => {
                return Err(self.fs_error("acquire maintenance lock", lock_directory, source));
            }
        }
        let lock_record = serde_json::json!({
            "schema_version": PROJECT_AGENT_SCHEMA_VERSION,
            "maintenance_id": maintenance_id,
            "agent_id": agent_id,
            "actor": actor,
            "started_at_ms": started_at_ms,
        });
        let owner_path = lock_directory.join("owner.json")?;
        if let Err(error) = file_system
            .write_file(
                &owner_path,
                serde_json::to_vec_pretty(&lock_record)?,
                sandbox(scope),
            )
            .await
            .map_err(|source| self.fs_error("write", &owner_path, source))
        {
            let _ = file_system
                .remove(
                    lock_directory,
                    RemoveOptions {
                        recursive: true,
                        force: true,
                    },
                    sandbox(scope),
                )
                .await;
            return Err(error);
        }
        Ok(())
    }
    pub(super) async fn load_memory_state(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<MemoryState, ProjectAgentMaintenanceError> {
        let index_path = RelativeProjectAgentPath::new("memory/index.toml")?;
        let resolved_index_path = self.maintenance_resolve_agent_path(agent_id, &index_path)?;
        let index = match self
            .read_optional_text(
                file_system,
                scope,
                agent_id,
                &index_path,
                MAX_MEMORY_INDEX_BYTES,
            )
            .await?
        {
            Some(contents) => {
                let index: ProjectAgentMemoryIndex =
                    toml::from_str(&contents).map_err(|source| {
                        ProjectAgentMaintenanceError::ParseMemoryIndex {
                            path: resolved_index_path,
                            source,
                        }
                    })?;
                index.validate()?;
                index
            }
            None => ProjectAgentMemoryIndex::default(),
        };
        let mut normalized = BTreeMap::new();
        let mut used_tokens = 0usize;
        for item in &index.items {
            let resolved_item_path = self.maintenance_resolve_agent_path(agent_id, item)?;
            let body = self
                .read_optional_text(file_system, scope, agent_id, item, MAX_MEMORY_ITEM_BYTES)
                .await?
                .ok_or_else(|| ProjectAgentMaintenanceError::AcceptedMemoryMissing {
                    path: resolved_item_path,
                })?;
            used_tokens = used_tokens.saturating_add(approx_tokens(&body));
            normalized.insert(normalize_body(&body), item.clone());
        }
        Ok(MemoryState {
            index,
            normalized,
            used_tokens,
        })
    }

    pub(super) async fn load_accepted_proposals(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
    ) -> Result<BTreeSet<String>, ProjectAgentMaintenanceError> {
        let directory = self
            .maintenance_resolve_agent_relative(agent_id, "proposals/accepted")?
            .1;
        let entries = match file_system.read_directory(&directory, sandbox(scope)).await {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
            Err(source) => return Err(self.fs_error("read", &directory, source)),
        };
        let mut accepted = BTreeSet::new();
        for entry in entries {
            if !entry.is_file || !entry.file_name.ends_with(".json") {
                continue;
            }
            let path = directory.join(&entry.file_name)?;
            let decision: ProjectAgentMaintenanceDecision = self
                .read_json_bounded(file_system, scope, &path, MAX_DECISION_BYTES)
                .await?;
            if decision.disposition == ProjectAgentMaintenanceDisposition::Accepted {
                accepted.insert(normalize_body(&decision.body));
            }
        }
        Ok(accepted)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn decision(
        &self,
        agent_id: &ProjectAgentId,
        actor: &str,
        maintenance_id: &str,
        pending: PendingItemFile,
        kind: ProjectAgentMaintenanceItemKind,
        disposition: ProjectAgentMaintenanceDisposition,
        evidence: Vec<String>,
        reason: String,
        accepted_path: Option<RelativeProjectAgentPath>,
        decided_at_ms: i64,
    ) -> Result<ProjectAgentMaintenanceDecision, ProjectAgentMaintenanceError> {
        let decision_path = match kind {
            ProjectAgentMaintenanceItemKind::MemoryCandidate => {
                RelativeProjectAgentPath::new(format!("memory/decisions/{}.json", pending.item_id))?
            }
            ProjectAgentMaintenanceItemKind::ImprovementProposal => {
                RelativeProjectAgentPath::new(format!(
                    "proposals/{}/{}.json",
                    disposition.as_str(),
                    pending.item_id
                ))?
            }
        };
        let accepted_path = accepted_path.or_else(|| {
            (kind == ProjectAgentMaintenanceItemKind::ImprovementProposal
                && disposition == ProjectAgentMaintenanceDisposition::Accepted)
                .then(|| decision_path.clone())
        });
        Ok(ProjectAgentMaintenanceDecision {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            maintenance_id: maintenance_id.to_string(),
            item_id: pending.item_id,
            kind,
            disposition,
            agent_id: agent_id.clone(),
            source_task_id: pending.item.task_id,
            source_path: pending.source_path,
            body: pending.item.body,
            evidence,
            reason,
            actor: actor.to_string(),
            decided_at_ms,
            decision_path,
            accepted_path,
        })
    }

    pub(super) async fn apply_decisions(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        memory_index: &ProjectAgentMemoryIndex,
        new_memory: &[(RelativeProjectAgentPath, String)],
        decisions: &[ProjectAgentMaintenanceDecision],
    ) -> Result<(), ProjectAgentMaintenanceError> {
        for directory in [
            "memory/facts",
            "memory/decisions",
            "proposals/accepted",
            "proposals/rejected",
            "maintenance/history",
        ] {
            let path = self
                .maintenance_resolve_agent_relative(agent_id, directory)?
                .1;
            file_system
                .create_directory(
                    &path,
                    CreateDirectoryOptions { recursive: true },
                    sandbox(scope),
                )
                .await
                .map_err(|source| self.fs_error("create", &path, source))?;
        }
        for (path, body) in new_memory {
            let resolved = self.maintenance_resolve_agent_path(agent_id, path)?;
            self.write_new(file_system, scope, &resolved, body.as_bytes().to_vec())
                .await?;
        }
        if !new_memory.is_empty()
            || decisions.iter().any(|decision| {
                decision.kind == ProjectAgentMaintenanceItemKind::MemoryCandidate
                    && decision.disposition == ProjectAgentMaintenanceDisposition::Accepted
            })
        {
            memory_index.validate()?;
            let index_path = self
                .maintenance_resolve_agent_relative(agent_id, "memory/index.toml")?
                .1;
            let contents = toml::to_string_pretty(memory_index).map_err(|source| {
                ProjectAgentMaintenanceError::SerializeMemoryIndex {
                    path: index_path.clone(),
                    source,
                }
            })?;
            file_system
                .write_file(&index_path, contents.into_bytes(), sandbox(scope))
                .await
                .map_err(|source| self.fs_error("write", &index_path, source))?;
        }
        for decision in decisions {
            let path = self.maintenance_resolve_agent_path(agent_id, &decision.decision_path)?;
            let contents = serde_json::to_vec_pretty(decision)?;
            if contents.len() > MAX_DECISION_BYTES {
                return Err(ProjectAgentMaintenanceError::SerializedArtifactTooLarge {
                    path,
                    actual: contents.len(),
                    maximum: MAX_DECISION_BYTES,
                });
            }
            self.write_new(file_system, scope, &path, contents).await?;
        }
        Ok(())
    }

    pub(super) async fn increment_catalog_revision(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
    ) -> Result<u64, ProjectAgentMaintenanceError> {
        let mut registry = self.load(file_system, scope).await?;
        registry.revision = registry.revision.saturating_add(1);
        self.write_registry(file_system, scope, &registry).await?;
        Ok(registry.revision)
    }

    pub(super) async fn write_registry(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        registry: &ProjectAgentRegistry,
    ) -> Result<(), ProjectAgentMaintenanceError> {
        registry.validate()?;
        let contents =
            toml::to_string_pretty(registry).map_err(ProjectAgentStoreError::SerializeRegistry)?;
        file_system
            .write_file(self.registry_path(), contents.into_bytes(), sandbox(scope))
            .await
            .map_err(|source| self.fs_error("write", self.registry_path(), source))
    }

    pub(super) async fn persist_report(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        report: &ProjectAgentMaintenanceReport,
    ) -> Result<(), ProjectAgentMaintenanceError> {
        let path = self
            .maintenance_resolve_agent_relative(
                &report.agent_id,
                &format!("maintenance/history/{}.json", report.maintenance_id),
            )?
            .1;
        let contents = serde_json::to_vec_pretty(report)?;
        if contents.len() > MAX_REPORT_BYTES {
            return Err(ProjectAgentMaintenanceError::SerializedArtifactTooLarge {
                path,
                actual: contents.len(),
                maximum: MAX_REPORT_BYTES,
            });
        }
        self.write_new(file_system, scope, &path, contents).await
    }

    pub(super) async fn read_optional_text(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        agent_id: &ProjectAgentId,
        path: &RelativeProjectAgentPath,
        maximum: usize,
    ) -> Result<Option<String>, ProjectAgentMaintenanceError> {
        let path = self.maintenance_resolve_agent_path(agent_id, path)?;
        match self
            .maintenance_read_text_bounded(file_system, scope, &path, maximum)
            .await
        {
            Ok(contents) => Ok(Some(contents)),
            Err(ProjectAgentMaintenanceError::FileSystem { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn read_json_bounded<T: DeserializeOwned>(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        maximum: usize,
    ) -> Result<T, ProjectAgentMaintenanceError> {
        let contents = self
            .maintenance_read_text_bounded(file_system, scope, path, maximum)
            .await?;
        serde_json::from_str(&contents).map_err(|source| ProjectAgentMaintenanceError::ParseJson {
            path: path.clone(),
            source,
        })
    }

    pub(super) async fn maintenance_read_text_bounded(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        maximum: usize,
    ) -> Result<String, ProjectAgentMaintenanceError> {
        let metadata = file_system
            .get_metadata(path, sandbox(scope))
            .await
            .map_err(|source| self.fs_error("inspect", path, source))?;
        if metadata.size > maximum as u64 {
            return Err(ProjectAgentMaintenanceError::FileTooLarge {
                path: path.clone(),
                actual: metadata.size,
                maximum,
            });
        }
        let contents = file_system
            .read_file(path, sandbox(scope))
            .await
            .map_err(|source| self.fs_error("read", path, source))?;
        if contents.len() > maximum {
            return Err(ProjectAgentMaintenanceError::FileTooLarge {
                path: path.clone(),
                actual: contents.len() as u64,
                maximum,
            });
        }
        String::from_utf8(contents).map_err(|source| {
            self.fs_error(
                "decode",
                path,
                io::Error::new(io::ErrorKind::InvalidData, source),
            )
        })
    }

    pub(super) async fn write_new(
        &self,
        file_system: &dyn ExecutorFileSystem,
        scope: ProjectAgentFileSystemScope<'_>,
        path: &PathUri,
        contents: Vec<u8>,
    ) -> Result<(), ProjectAgentMaintenanceError> {
        match file_system.get_metadata(path, sandbox(scope)).await {
            Ok(_) => {
                return Err(ProjectAgentMaintenanceError::ArtifactAlreadyExists(
                    path.clone(),
                ));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(self.fs_error("inspect", path, source)),
        }
        file_system
            .write_file(path, contents, sandbox(scope))
            .await
            .map_err(|source| self.fs_error("write", path, source))
    }

    pub(super) fn maintenance_resolve_agent_relative(
        &self,
        agent_id: &ProjectAgentId,
        path: &str,
    ) -> Result<(RelativeProjectAgentPath, PathUri), ProjectAgentMaintenanceError> {
        let relative = RelativeProjectAgentPath::new(path)?;
        let resolved = self.maintenance_resolve_agent_path(agent_id, &relative)?;
        Ok((relative, resolved))
    }

    pub(super) fn maintenance_resolve_agent_path(
        &self,
        agent_id: &ProjectAgentId,
        path: &RelativeProjectAgentPath,
    ) -> Result<PathUri, ProjectAgentMaintenanceError> {
        Ok(self.resolve(&RelativeProjectAgentPath::new(format!(
            "agents/{agent_id}/{path}"
        ))?)?)
    }

    pub(super) fn fs_error(
        &self,
        operation: &'static str,
        path: &PathUri,
        source: io::Error,
    ) -> ProjectAgentMaintenanceError {
        ProjectAgentMaintenanceError::FileSystem {
            operation,
            path: path.clone(),
            source,
        }
    }
}
