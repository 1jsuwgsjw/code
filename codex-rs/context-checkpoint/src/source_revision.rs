use crate::CheckpointError;
use crate::ContextStateSnapshot;
use crate::EvidenceRef;
use crate::StateEntry;
use crate::StateEntryKind;
use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;

const MAX_SOURCE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SOURCE_STATE_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) async fn invalidate_stale_source_facts(
    state: &mut ContextStateSnapshot,
    workspace_root: &Path,
) {
    let mut inspected_bytes = 0;
    retain_current_source_facts(&mut state.quarter, workspace_root, &mut inspected_bytes).await;
    retain_current_source_facts(&mut state.session, workspace_root, &mut inspected_bytes).await;
    retain_current_source_facts(
        &mut state.active.entries,
        workspace_root,
        &mut inspected_bytes,
    )
    .await;
}

pub(crate) async fn stamp_source_facts(
    state: &mut ContextStateSnapshot,
    workspace_root: &Path,
) -> Result<(), CheckpointError> {
    let mut inspected_bytes = 0;
    stamp_entries(&mut state.quarter, workspace_root, &mut inspected_bytes).await?;
    stamp_entries(&mut state.session, workspace_root, &mut inspected_bytes).await?;
    stamp_entries(
        &mut state.active.entries,
        workspace_root,
        &mut inspected_bytes,
    )
    .await
}

async fn retain_current_source_facts(
    entries: &mut Vec<StateEntry>,
    workspace_root: &Path,
    inspected_bytes: &mut u64,
) {
    let mut retained = Vec::with_capacity(entries.len());
    for mut entry in entries.drain(..) {
        if entry.kind != StateEntryKind::SourceFact {
            retained.push(entry);
            continue;
        }

        match source_revision(&entry, workspace_root, inspected_bytes).await {
            Ok(None) => {
                entry.source_revision = None;
                retained.push(entry);
            }
            Ok(Some(revision))
                if entry
                    .source_revision
                    .as_ref()
                    .is_none_or(|stored| stored == &revision) =>
            {
                entry.source_revision = Some(revision);
                retained.push(entry);
            }
            Ok(Some(_)) | Err(_) => {}
        }
    }
    *entries = retained;
}

async fn stamp_entries(
    entries: &mut [StateEntry],
    workspace_root: &Path,
    inspected_bytes: &mut u64,
) -> Result<(), CheckpointError> {
    for entry in entries {
        if entry.kind != StateEntryKind::SourceFact {
            continue;
        }
        entry.source_revision = source_revision(entry, workspace_root, inspected_bytes)
            .await
            .map_err(CheckpointError::UntrustedEvidence)?;
    }
    Ok(())
}

async fn source_revision(
    entry: &StateEntry,
    workspace_root: &Path,
    inspected_bytes: &mut u64,
) -> Result<Option<String>, String> {
    let mut sources = entry
        .evidence
        .iter()
        .filter_map(|evidence| match evidence {
            EvidenceRef::WorkspacePath { path } => Some(path.as_str()),
            EvidenceRef::Artifact { .. } | EvidenceRef::Symbol { .. } => None,
        })
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    if sources.is_empty() {
        return Ok(None);
    }

    let mut digest = Sha256::new();
    for source in sources {
        let source_path = PathBuf::from(source);
        let resolved = if source_path.is_absolute() {
            source_path
        } else {
            workspace_root.join(source_path)
        };
        let metadata = tokio::fs::metadata(&resolved)
            .await
            .map_err(|error| format!("cannot inspect source evidence {source}: {error}"))?;
        if !metadata.is_file() {
            return Err(format!("source evidence is not a file: {source}"));
        }
        if metadata.len() > MAX_SOURCE_FILE_BYTES {
            return Err(format!(
                "source evidence exceeds {MAX_SOURCE_FILE_BYTES} bytes: {source}"
            ));
        }
        *inspected_bytes = inspected_bytes.saturating_add(metadata.len());
        if *inspected_bytes > MAX_SOURCE_STATE_BYTES {
            return Err(format!(
                "source evidence state exceeds {MAX_SOURCE_STATE_BYTES} bytes"
            ));
        }
        let bytes = tokio::fs::read(&resolved)
            .await
            .map_err(|error| format!("cannot read source evidence {source}: {error}"))?;
        digest.update((source.len() as u64).to_le_bytes());
        digest.update(source.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }

    Ok(Some(format!("sha256:{:x}", digest.finalize())))
}

#[cfg(test)]
#[path = "source_revision_tests.rs"]
mod tests;
