use super::*;
use codex_file_system::CopyOptions;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystem;
use codex_file_system::ExecutorFileSystemFuture;
use codex_file_system::FileMetadata;
use codex_file_system::FileSystemReadStream;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::ReadDirectoryEntry;
use codex_file_system::RemoveOptions;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use std::io;
use tempfile::tempdir;

struct TestFileSystem;

impl ExecutorFileSystem for TestFileSystem {
    fn canonicalize<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, PathUri> {
        Box::pin(async move {
            let path = path.to_abs_path()?.canonicalize()?;
            Ok(PathUri::from_abs_path(&path))
        })
    }

    fn read_file<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<u8>> {
        Box::pin(async move { tokio::fs::read(path.to_abs_path()?.as_path()).await })
    }

    fn read_file_stream<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileSystemReadStream> {
        Box::pin(async { Err(io::Error::from(io::ErrorKind::Unsupported)) })
    }

    fn write_file<'a>(
        &'a self,
        path: &'a PathUri,
        contents: Vec<u8>,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { tokio::fs::write(path.to_abs_path()?.as_path(), contents).await })
    }

    fn create_directory<'a>(
        &'a self,
        path: &'a PathUri,
        options: CreateDirectoryOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            if options.recursive {
                tokio::fs::create_dir_all(path.as_path()).await
            } else {
                tokio::fs::create_dir(path.as_path()).await
            }
        })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileMetadata> {
        Box::pin(async move {
            let metadata = tokio::fs::symlink_metadata(path.to_abs_path()?.as_path()).await?;
            let file_type = metadata.file_type();
            Ok(FileMetadata {
                is_directory: file_type.is_dir(),
                is_file: file_type.is_file(),
                is_symlink: file_type.is_symlink(),
                size: metadata.len(),
                created_at_ms: 0,
                modified_at_ms: 0,
            })
        })
    }

    fn read_directory<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<ReadDirectoryEntry>> {
        Box::pin(async { Err(io::Error::from(io::ErrorKind::Unsupported)) })
    }

    fn remove<'a>(
        &'a self,
        _path: &'a PathUri,
        _options: RemoveOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async { Err(io::Error::from(io::ErrorKind::Unsupported)) })
    }

    fn copy<'a>(
        &'a self,
        _source_path: &'a PathUri,
        _destination_path: &'a PathUri,
        _options: CopyOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async { Err(io::Error::from(io::ErrorKind::Unsupported)) })
    }
}

#[tokio::test]
async fn bootstrap_resolves_root_and_preserves_existing_registry() {
    let temp_dir = tempdir().expect("tempdir");
    let project_root = AbsolutePathBuf::try_from(temp_dir.path()).expect("absolute temp path");
    let nested = project_root.join("nested/work");
    std::fs::create_dir_all(nested.as_path()).expect("nested cwd");
    std::fs::create_dir(project_root.join(".git").as_path()).expect("project marker");

    let file_system = TestFileSystem;
    let cwd = PathUri::from_abs_path(&nested);
    let marker = RelativeProjectAgentPath::new(".git").expect("marker");
    let resolved = resolve_project_root(
        &file_system,
        &cwd,
        &[marker],
        ProjectAgentFileSystemScope::Unrestricted,
    )
    .await
    .expect("project root");
    assert_eq!(resolved, PathUri::from_abs_path(&project_root));

    let store = ProjectAgentStore::new(resolved).expect("store");
    let first = store
        .bootstrap(&file_system, ProjectAgentFileSystemScope::Unrestricted)
        .await
        .expect("initial bootstrap");
    assert_eq!(
        first,
        BootstrapOutcome {
            disposition: BootstrapDisposition::Created,
            registry: ProjectAgentRegistry::default(),
        }
    );

    let agent_id = ProjectAgentId::new("query").expect("agent id");
    let registry = ProjectAgentRegistry {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agents: BTreeMap::from([(
            agent_id,
            ProjectAgentRegistration {
                path: RelativeProjectAgentPath::new("agents/query/agent.toml")
                    .expect("definition path"),
                enabled: true,
            },
        )]),
    };
    let preserved = toml::to_string_pretty(&registry).expect("registry TOML");
    std::fs::write(
        store
            .registry_path()
            .to_abs_path()
            .expect("local registry path")
            .as_path(),
        &preserved,
    )
    .expect("replace registry fixture");

    let second = store
        .bootstrap(&file_system, ProjectAgentFileSystemScope::Unrestricted)
        .await
        .expect("repeated bootstrap");
    assert_eq!(
        second,
        BootstrapOutcome {
            disposition: BootstrapDisposition::Existing,
            registry,
        }
    );
    assert_eq!(
        std::fs::read_to_string(
            store
                .registry_path()
                .to_abs_path()
                .expect("local registry path")
                .as_path()
        )
        .expect("read preserved registry"),
        preserved
    );
}

#[test]
fn manifests_and_results_enforce_declared_shapes() {
    let manifest: ProjectAgentToolManifest = toml::from_str(
        r#"
schema_version = 1
id = "search"
description = "Runs bounded project search."
kind = "command"
program = "tools/search.py"
timeout_ms = 30000
input_schema = "tools/search.schema.json"
"#,
    )
    .expect("tool manifest");
    manifest.validate().expect("valid tool manifest");
    assert_eq!(
        manifest,
        ProjectAgentToolManifest {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: ProjectAgentId::new("search").expect("tool id"),
            description: "Runs bounded project search.".to_string(),
            target: ProjectAgentToolTarget::Command {
                program: RelativeProjectAgentPath::new("tools/search.py").expect("program path"),
            },
            timeout_ms: Some(30_000),
            input_schema: Some(
                RelativeProjectAgentPath::new("tools/search.schema.json")
                    .expect("input schema path"),
            ),
        }
    );

    let result = serde_json::from_str::<ProjectAgentTaskResult>(
        r#"{"status":"completed","agent_id":"query","task_id":"T-1","result":"ok","artifacts":[],"evidence":[],"memory_candidates":[],"improvement_proposals":[],"error":null}"#,
    )
    .expect("task result");
    assert_eq!(
        result,
        ProjectAgentTaskResult {
            status: ProjectAgentTaskStatus::Completed,
            agent_id: ProjectAgentId::new("query").expect("agent id"),
            task_id: "T-1".to_string(),
            result: "ok".to_string(),
            artifacts: Vec::new(),
            evidence: Vec::new(),
            memory_candidates: Vec::new(),
            improvement_proposals: Vec::new(),
            error: None,
        }
    );

    let missing_error = serde_json::from_str::<ProjectAgentTaskResult>(
        r#"{"status":"completed","agent_id":"query","task_id":"T-1","result":"ok","artifacts":[],"evidence":[],"memory_candidates":[],"improvement_proposals":[]}"#,
    )
    .expect_err("error field is required");
    assert!(missing_error.to_string().contains("error"));
    assert!(RelativeProjectAgentPath::new("../outside").is_err());
}

#[test]
fn definitions_and_registry_paths_are_validated() {
    let definition: ProjectAgentDefinition = toml::from_str(
        r#"
schema_version = 1
id = "query"
description = "Performs bounded repository queries."
constraints_file = "constraints.md"
model = "gpt-5.6-luna"
model_reasoning_effort = "medium"
model_provider = "default"
tools = ["tools/search.x"]
memory_max_items = 32
memory_max_tokens = 4000
"#,
    )
    .expect("agent definition");
    definition.validate().expect("valid agent definition");
    assert_eq!(
        definition,
        ProjectAgentDefinition {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: ProjectAgentId::new("query").expect("agent id"),
            description: "Performs bounded repository queries.".to_string(),
            constraints_file: RelativeProjectAgentPath::new("constraints.md")
                .expect("constraints path"),
            model: Some("gpt-5.6-luna".to_string()),
            model_reasoning_effort: Some("medium".to_string()),
            model_provider: Some("default".to_string()),
            tools: vec![
                RelativeProjectAgentPath::new("tools/search.x").expect("tool manifest path"),
            ],
            memory_max_items: 32,
            memory_max_tokens: 4_000,
        }
    );

    let agent_id = ProjectAgentId::new("query").expect("agent id");
    let registry = ProjectAgentRegistry {
        schema_version: PROJECT_AGENT_SCHEMA_VERSION,
        agents: BTreeMap::from([(
            agent_id.clone(),
            ProjectAgentRegistration {
                path: RelativeProjectAgentPath::new("agents/other/agent.toml")
                    .expect("registry path"),
                enabled: true,
            },
        )]),
    };
    assert_eq!(
        registry.validate().expect_err("mismatched registry path"),
        ProjectAgentValidationError::RegistryPathMismatch {
            agent_id,
            expected: "agents/query/agent.toml".to_string(),
            actual: "agents/other/agent.toml".to_string(),
        }
    );
}
