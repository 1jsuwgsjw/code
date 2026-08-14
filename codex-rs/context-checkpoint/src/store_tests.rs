use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::tempdir;

fn test_store() -> (tempfile::TempDir, CheckpointStore) {
    let directory = tempdir().expect("create temporary checkpoint directory");
    let root = AbsolutePathBuf::try_from(directory.path().to_path_buf())
        .expect("temporary directory is absolute");
    (directory, CheckpointStore::new(root))
}

#[tokio::test]
async fn artifact_round_trip_is_content_addressed_and_bounded() {
    let (_directory, store) = test_store();
    let artifact = store
        .write_artifact("text/plain", b"checkpoint evidence")
        .await
        .expect("write artifact");

    let (data, truncated) = store
        .recall_artifact(&artifact, 10)
        .await
        .expect("recall artifact");

    assert_eq!(data, b"checkpoint".to_vec());
    assert!(truncated);
    assert_eq!(artifact.artifact_id.as_str(), artifact.sha256);
}

#[tokio::test]
async fn artifact_hash_mismatch_is_rejected() {
    let (_directory, store) = test_store();
    let artifact = store
        .write_artifact("text/plain", b"trusted")
        .await
        .expect("write artifact");
    let path = store
        .artifact_path(&artifact.artifact_id)
        .expect("artifact path");
    tokio::fs::write(path.as_path(), b"changed")
        .await
        .expect("tamper artifact");

    let error = store
        .verify_artifact(&artifact)
        .await
        .expect_err("tampered artifact must fail");

    assert!(matches!(error, CheckpointError::UntrustedEvidence(_)));
}

#[tokio::test]
async fn manifest_replacement_and_previous_file_recovery_are_supported() {
    let (_directory, store) = test_store();
    store
        .write_manifest(&json!({"version": 1}))
        .await
        .expect("write first manifest");
    store
        .write_manifest(&json!({"version": 2}))
        .await
        .expect("replace manifest");
    let current: serde_json::Value = store
        .load_manifest()
        .await
        .expect("load current manifest")
        .expect("manifest exists");
    assert_eq!(current, json!({"version": 2}));

    let manifest = store.root.join(MANIFEST_FILE);
    let previous = store.root.join(PREVIOUS_MANIFEST_FILE);
    tokio::fs::rename(manifest.as_path(), previous.as_path())
        .await
        .expect("simulate interrupted replacement");
    let recovered: serde_json::Value = store
        .load_manifest()
        .await
        .expect("recover previous manifest")
        .expect("previous manifest exists");

    assert_eq!(recovered, json!({"version": 2}));
}
