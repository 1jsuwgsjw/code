use crate::ArtifactId;
use crate::ArtifactRef;
use crate::CheckpointError;
use crate::CheckpointGenerationId;
use crate::TurnRecord;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::Digest;
use sha2::Sha256;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;

const MANIFEST_FILE: &str = "manifest.json";
const PREVIOUS_MANIFEST_FILE: &str = "manifest.previous.json";
const MANIFESTS_DIR: &str = "manifests";

#[derive(Clone, Debug)]
pub struct CheckpointStore {
    root: AbsolutePathBuf,
    temporary_sequence: Arc<AtomicU64>,
}

impl CheckpointStore {
    pub fn new(root: AbsolutePathBuf) -> Self {
        Self {
            root,
            temporary_sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn root(&self) -> &AbsolutePathBuf {
        &self.root
    }

    pub fn sibling_thread_store(&self, thread_id: &str) -> Result<Self, CheckpointError> {
        let thread_path = Path::new(thread_id);
        if thread_id.is_empty()
            || thread_path.components().count() != 1
            || thread_path.file_name().and_then(|name| name.to_str()) != Some(thread_id)
        {
            return Err(CheckpointError::InvalidRequest(format!(
                "invalid checkpoint source thread id {thread_id:?}"
            )));
        }
        let parent = self.root.as_path().parent().ok_or_else(|| {
            CheckpointError::Storage("checkpoint store has no parent directory".to_string())
        })?;
        let root = AbsolutePathBuf::try_from(parent.join(thread_path))
            .map_err(|error| CheckpointError::Storage(error.to_string()))?;
        Ok(Self::new(root))
    }

    pub async fn load_manifest<T: DeserializeOwned>(&self) -> Result<Option<T>, CheckpointError> {
        let manifest_path = self.root.join(MANIFEST_FILE);
        let bytes = match tokio::fs::read(manifest_path.as_path()).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let previous_path = self.root.join(PREVIOUS_MANIFEST_FILE);
                match tokio::fs::read(previous_path.as_path()).await {
                    Ok(bytes) => bytes,
                    Err(previous_error)
                        if previous_error.kind() == std::io::ErrorKind::NotFound =>
                    {
                        return Ok(None);
                    }
                    Err(previous_error) => return Err(storage_error(previous_error)),
                }
            }
            Err(error) => return Err(storage_error(error)),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| CheckpointError::Storage(error.to_string()))
    }

    pub async fn write_manifest<T: Serialize>(
        &self,
        manifest: &T,
    ) -> Result<String, CheckpointError> {
        let bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|error| CheckpointError::Storage(error.to_string()))?;
        let sha256 = sha256_hex(&bytes);
        let snapshot_path = self.manifest_snapshot_path(&sha256)?;
        self.write_immutable(snapshot_path.as_path(), &bytes)
            .await?;
        self.write_replaceable(MANIFEST_FILE, &bytes).await?;
        Ok(sha256)
    }

    pub(crate) async fn load_manifest_by_sha256<T: DeserializeOwned>(
        &self,
        expected_sha256: &str,
    ) -> Result<T, CheckpointError> {
        let path = self.manifest_snapshot_path(expected_sha256)?;
        let bytes = tokio::fs::read(path.as_path()).await.map_err(|error| {
            CheckpointError::Storage(format!(
                "failed to read checkpoint manifest {expected_sha256}: {error}"
            ))
        })?;
        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != expected_sha256 {
            return Err(CheckpointError::UntrustedEvidence(format!(
                "checkpoint manifest hash mismatch: expected {expected_sha256}, got {actual_sha256}"
            )));
        }
        serde_json::from_slice(&bytes).map_err(|error| CheckpointError::Storage(error.to_string()))
    }

    pub async fn current_manifest_sha256(&self) -> Result<Option<String>, CheckpointError> {
        let path = self.root.join(MANIFEST_FILE);
        match tokio::fs::read(path.as_path()).await {
            Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(storage_error(error)),
        }
    }

    pub async fn write_artifact(
        &self,
        media_type: impl Into<String>,
        data: &[u8],
    ) -> Result<ArtifactRef, CheckpointError> {
        let sha256 = sha256_hex(data);
        let artifact_id = ArtifactId::from_sha256(sha256.clone());
        let path = self.artifact_path(&artifact_id)?;
        self.write_immutable(path.as_path(), data).await?;
        Ok(ArtifactRef {
            artifact_id,
            sha256,
            byte_len: data.len() as u64,
            media_type: media_type.into(),
        })
    }

    pub async fn verify_artifact(&self, artifact: &ArtifactRef) -> Result<(), CheckpointError> {
        let data = self.read_verified_artifact(artifact).await?;
        if data.len() as u64 != artifact.byte_len {
            return Err(CheckpointError::UntrustedEvidence(format!(
                "artifact {} length differs from its manifest",
                artifact.artifact_id
            )));
        }
        Ok(())
    }

    pub(crate) async fn copy_artifact_from(
        &self,
        source: &Self,
        artifact: &ArtifactRef,
    ) -> Result<(), CheckpointError> {
        let data = source.read_verified_artifact(artifact).await?;
        let copied = self
            .write_artifact(artifact.media_type.clone(), &data)
            .await?;
        if copied != *artifact {
            return Err(CheckpointError::UntrustedEvidence(format!(
                "copied artifact {} changed identity",
                artifact.artifact_id
            )));
        }
        Ok(())
    }

    pub async fn recall_artifact(
        &self,
        artifact: &ArtifactRef,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, bool), CheckpointError> {
        let data = self.read_verified_artifact(artifact).await?;
        let truncated = data.len() > max_bytes;
        Ok((data.into_iter().take(max_bytes).collect(), truncated))
    }

    pub async fn write_turn_record(&self, record: &TurnRecord) -> Result<String, CheckpointError> {
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|error| CheckpointError::Storage(error.to_string()))?;
        let relative = PathBuf::from("turns")
            .join(record.generation_id.to_string())
            .join(format!("{}.json", record.record_id));
        let path = self.root.join(relative);
        self.write_immutable(path.as_path(), &bytes).await?;
        Ok(sha256_hex(&bytes))
    }

    fn artifact_path(&self, artifact_id: &ArtifactId) -> Result<AbsolutePathBuf, CheckpointError> {
        let id = artifact_id.as_str();
        let prefix = id.get(..2).ok_or_else(|| {
            CheckpointError::Storage(format!("invalid artifact id: {artifact_id}"))
        })?;
        Ok(self
            .root
            .join("artifacts")
            .join(prefix)
            .join(format!("{id}.blob")))
    }

    fn manifest_snapshot_path(&self, sha256: &str) -> Result<AbsolutePathBuf, CheckpointError> {
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CheckpointError::InvalidRequest(format!(
                "invalid checkpoint manifest hash {sha256:?}"
            )));
        }
        Ok(self.root.join(MANIFESTS_DIR).join(format!("{sha256}.json")))
    }

    async fn read_verified_artifact(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<Vec<u8>, CheckpointError> {
        let path = self.artifact_path(&artifact.artifact_id)?;
        let data = tokio::fs::read(path.as_path()).await.map_err(|error| {
            CheckpointError::UntrustedEvidence(format!(
                "artifact {} cannot be read: {error}",
                artifact.artifact_id
            ))
        })?;
        let actual = sha256_hex(&data);
        if actual != artifact.sha256 || actual != artifact.artifact_id.as_str() {
            return Err(CheckpointError::UntrustedEvidence(format!(
                "artifact {} hash mismatch",
                artifact.artifact_id
            )));
        }
        Ok(data)
    }

    async fn write_immutable(&self, path: &Path, bytes: &[u8]) -> Result<(), CheckpointError> {
        if tokio::fs::try_exists(path).await.map_err(storage_error)? {
            let existing = tokio::fs::read(path).await.map_err(storage_error)?;
            if existing == bytes {
                return Ok(());
            }
            return Err(CheckpointError::Storage(format!(
                "immutable checkpoint file already exists with different content: {}",
                path.display()
            )));
        }
        let temporary = self.write_temporary(path, bytes).await?;
        match tokio::fs::rename(&temporary, path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = tokio::fs::remove_file(&temporary).await;
                let existing = tokio::fs::read(path).await.map_err(storage_error)?;
                if existing == bytes {
                    Ok(())
                } else {
                    Err(CheckpointError::Storage(format!(
                        "immutable checkpoint race produced different content: {}",
                        path.display()
                    )))
                }
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                Err(storage_error(error))
            }
        }
    }

    async fn write_replaceable(&self, relative: &str, bytes: &[u8]) -> Result<(), CheckpointError> {
        tokio::fs::create_dir_all(self.root.as_path())
            .await
            .map_err(storage_error)?;
        let path = self.root.join(relative);
        let temporary = self.write_temporary(path.as_path(), bytes).await?;
        if tokio::fs::rename(&temporary, path.as_path()).await.is_ok() {
            return Ok(());
        }

        let previous = self.root.join(PREVIOUS_MANIFEST_FILE);
        if tokio::fs::try_exists(previous.as_path())
            .await
            .map_err(storage_error)?
        {
            tokio::fs::remove_file(previous.as_path())
                .await
                .map_err(storage_error)?;
        }
        if tokio::fs::try_exists(path.as_path())
            .await
            .map_err(storage_error)?
        {
            tokio::fs::rename(path.as_path(), previous.as_path())
                .await
                .map_err(storage_error)?;
        }
        if let Err(error) = tokio::fs::rename(&temporary, path.as_path()).await {
            if tokio::fs::try_exists(previous.as_path())
                .await
                .unwrap_or(false)
            {
                let _ = tokio::fs::rename(previous.as_path(), path.as_path()).await;
            }
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(storage_error(error));
        }
        let _ = tokio::fs::remove_file(previous.as_path()).await;
        Ok(())
    }

    async fn write_temporary(&self, path: &Path, bytes: &[u8]) -> Result<PathBuf, CheckpointError> {
        let parent = path.parent().ok_or_else(|| {
            CheckpointError::Storage(format!("checkpoint path has no parent: {}", path.display()))
        })?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(storage_error)?;
        let sequence = self.temporary_sequence.fetch_add(1, Ordering::Relaxed);
        let mut name = path
            .file_name()
            .map(OsString::from)
            .unwrap_or_else(|| OsString::from("checkpoint"));
        name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let temporary = parent.join(name);
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await
            .map_err(storage_error)?;
        file.write_all(bytes).await.map_err(storage_error)?;
        file.flush().await.map_err(storage_error)?;
        file.sync_all().await.map_err(storage_error)?;
        drop(file);
        Ok(temporary)
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn content_sha256(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

fn storage_error(error: std::io::Error) -> CheckpointError {
    CheckpointError::Storage(error.to_string())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
