use crate::ArtifactId;
use crate::CheckpointError;
use crate::CheckpointGenerationId;
use crate::CheckpointRuntime;
use crate::InstalledCheckpoint;
use crate::runtime::MANIFEST_SCHEMA_VERSION;
use crate::runtime::RuntimeManifest;
use std::collections::HashSet;

impl CheckpointRuntime {
    pub async fn reconcile_reconstructed_checkpoint(
        &self,
        source_thread_id: &str,
        expected_manifest_sha256: &str,
        expected_generation_id: CheckpointGenerationId,
        installed: InstalledCheckpoint,
    ) -> Result<(), CheckpointError> {
        let source_store = self.store.sibling_thread_store(source_thread_id)?;
        let snapshot = source_store
            .load_manifest_by_sha256::<RuntimeManifest>(expected_manifest_sha256)
            .await?;
        validate_snapshot(&snapshot, expected_generation_id, &installed)?;

        let already_installed = {
            let state = self.state.lock().await;
            state.manifest.last_installed.as_ref() == Some(&installed)
                && state
                    .manifest
                    .turn_records
                    .iter()
                    .any(|record| record.record_id == installed.record_id)
        };
        if already_installed {
            return Ok(());
        }

        if source_store.root() != self.store.root() {
            copy_snapshot_dependencies(&self.store, &source_store, &snapshot).await?;
        }

        let manifest_sha256 = self.store.write_manifest(&snapshot).await?;
        {
            let mut state = self.state.lock().await;
            state.manifest = snapshot;
            state.manifest_sha256 = Some(manifest_sha256);
            state.degraded_reason = None;
        }
        self.mark_installed(installed).await
    }

    pub async fn clear_reconstructed_checkpoint(&self) -> Result<(), CheckpointError> {
        let should_clear = {
            let state = self.state.lock().await;
            state.manifest.last_installed.is_some() || state.manifest.pending_checkpoint.is_some()
        };
        if !should_clear {
            return Ok(());
        }
        {
            let mut state = self.state.lock().await;
            state.manifest = RuntimeManifest::empty();
            state.manifest_sha256 = None;
            state.degraded_reason = None;
        }
        self.persist().await?;
        Ok(())
    }
}

fn validate_snapshot(
    snapshot: &RuntimeManifest,
    expected_generation_id: CheckpointGenerationId,
    installed: &InstalledCheckpoint,
) -> Result<(), CheckpointError> {
    if snapshot.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(CheckpointError::Storage(format!(
            "unsupported checkpoint manifest schema {}",
            snapshot.schema_version
        )));
    }
    if snapshot.generation_id != expected_generation_id {
        return Err(CheckpointError::UntrustedEvidence(format!(
            "checkpoint generation mismatch: expected {expected_generation_id}, got {}",
            snapshot.generation_id
        )));
    }
    let pending = snapshot
        .pending_checkpoint
        .as_ref()
        .ok_or(CheckpointError::NoPendingCheckpoint)?;
    if pending.record.record_id != installed.record_id
        || pending.record.generation_id != expected_generation_id
    {
        return Err(CheckpointError::UntrustedEvidence(format!(
            "checkpoint record {} does not match generation {expected_generation_id}",
            installed.record_id
        )));
    }
    if !snapshot
        .turn_records
        .iter()
        .any(|record| record.record_id == installed.record_id)
    {
        return Err(CheckpointError::UntrustedEvidence(format!(
            "checkpoint record {} is missing from its manifest",
            installed.record_id
        )));
    }
    Ok(())
}

async fn copy_snapshot_dependencies(
    target: &crate::CheckpointStore,
    source: &crate::CheckpointStore,
    snapshot: &RuntimeManifest,
) -> Result<(), CheckpointError> {
    let mut copied = HashSet::<ArtifactId>::new();
    let indexed_artifacts = snapshot.artifacts.iter().chain(
        snapshot
            .tool_groups
            .iter()
            .flat_map(|group| &group.calls)
            .flat_map(|call| [&call.input, &call.output]),
    );
    for artifact in indexed_artifacts {
        if copied.insert(artifact.artifact_id.clone()) {
            target.copy_artifact_from(source, artifact).await?;
        }
    }
    for record in &snapshot.turn_records {
        target.write_turn_record(record).await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "reconcile_tests.rs"]
mod tests;
