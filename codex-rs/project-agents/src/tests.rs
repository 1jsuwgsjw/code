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
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<ReadDirectoryEntry>> {
        Box::pin(async move {
            let mut read_dir = tokio::fs::read_dir(path.to_abs_path()?.as_path()).await?;
            let mut entries = Vec::new();
            while let Some(entry) = read_dir.next_entry().await? {
                let file_type = entry.file_type().await?;
                entries.push(ReadDirectoryEntry {
                    file_name: entry.file_name().to_string_lossy().into_owned(),
                    is_directory: file_type.is_dir(),
                    is_file: file_type.is_file(),
                });
            }
            Ok(entries)
        })
    }

    fn remove<'a>(
        &'a self,
        path: &'a PathUri,
        options: RemoveOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            let metadata = match tokio::fs::symlink_metadata(path.as_path()).await {
                Ok(metadata) => metadata,
                Err(error) if options.force && error.kind() == io::ErrorKind::NotFound => {
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            if metadata.is_dir() {
                if options.recursive {
                    tokio::fs::remove_dir_all(path.as_path()).await
                } else {
                    tokio::fs::remove_dir(path.as_path()).await
                }
            } else {
                tokio::fs::remove_file(path.as_path()).await
            }
        })
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
        revision: 0,
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
    result
        .validate_for(&ProjectAgentId::new("query").expect("agent id"), "T-1")
        .expect("valid task result");
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

#[tokio::test]
async fn runtime_loading_and_result_persistence_are_bounded_and_structured() {
    let temp_dir = tempdir().expect("tempdir");
    let project_root = AbsolutePathBuf::try_from(temp_dir.path()).expect("absolute temp path");
    let store = ProjectAgentStore::new(PathUri::from_abs_path(&project_root)).expect("store");
    let file_system = TestFileSystem;
    let scope = ProjectAgentFileSystemScope::Unrestricted;
    let agent_id = ProjectAgentId::new("query").expect("agent id");
    let mut entry = store
        .create(
            &file_system,
            scope,
            agent_id.clone(),
            "Performs bounded repository queries.".to_string(),
        )
        .await
        .expect("create agent");
    entry.definition.tools =
        vec![RelativeProjectAgentPath::new("tools/search.x").expect("manifest path")];

    let agent_root = project_root.join("AGENT/agents/query");
    std::fs::create_dir_all(agent_root.join("tools").as_path()).expect("tools directory");
    std::fs::create_dir_all(agent_root.join("memory/facts").as_path()).expect("memory directory");
    std::fs::write(
        agent_root.join("agent.toml").as_path(),
        toml::to_string_pretty(&entry.definition).expect("definition TOML"),
    )
    .expect("definition");
    std::fs::write(
        agent_root.join("tools/search.x").as_path(),
        r#"schema_version = 1
id = "search"
description = "Searches the project."
kind = "command"
program = "tools/search.py"
input_schema = "tools/search.schema.json"
"#,
    )
    .expect("manifest");
    std::fs::write(
        agent_root.join("tools/search.schema.json").as_path(),
        r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}"#,
    )
    .expect("schema");
    std::fs::write(
        agent_root.join("memory/index.toml").as_path(),
        "schema_version = 1\nitems = [\"memory/facts/one.md\"]\n",
    )
    .expect("memory index");
    std::fs::write(
        agent_root.join("memory/facts/one.md").as_path(),
        "Use the focused repository navigation route.",
    )
    .expect("memory item");

    let runtime = store
        .load_runtime(&file_system, scope, &agent_id)
        .await
        .expect("runtime");
    assert_eq!(runtime.entry, entry);
    assert_eq!(runtime.tools.len(), 1);
    assert_eq!(
        runtime.accepted_memory,
        vec!["Use the focused repository navigation route.".to_string()]
    );

    let result = ProjectAgentTaskResult {
        status: ProjectAgentTaskStatus::Completed,
        agent_id,
        task_id: "turn-1-call-1".to_string(),
        result: "Located the requested symbol.".to_string(),
        artifacts: Vec::new(),
        evidence: vec!["src/lib.rs:42".to_string()],
        memory_candidates: vec!["The entrypoint is src/lib.rs.".to_string()],
        improvement_proposals: vec!["Add a narrower search helper.".to_string()],
        error: None,
    };
    let persisted = store
        .persist_result(&file_system, scope, &result)
        .await
        .expect("persist result");
    assert!(
        persisted
            .history_path
            .to_abs_path()
            .expect("history path")
            .as_path()
            .is_file()
    );
    assert_eq!(persisted.memory_candidate_paths.len(), 1);
    assert_eq!(persisted.improvement_proposal_paths.len(), 1);
    assert!(matches!(
        store.persist_result(&file_system, scope, &result).await,
        Err(ProjectAgentStoreError::ArtifactAlreadyExists(_))
    ));
}

#[tokio::test]
async fn maintenance_dry_run_and_apply_are_attributable_and_deduplicated() {
    let temp_dir = tempdir().expect("tempdir");
    let project_root = AbsolutePathBuf::try_from(temp_dir.path()).expect("absolute temp path");
    let store = ProjectAgentStore::new(PathUri::from_abs_path(&project_root)).expect("store");
    let file_system = TestFileSystem;
    let scope = ProjectAgentFileSystemScope::Unrestricted;
    let agent_id = ProjectAgentId::new("query").expect("agent id");
    store
        .create(
            &file_system,
            scope,
            agent_id.clone(),
            "Performs bounded repository queries.".to_string(),
        )
        .await
        .expect("create agent");

    let results = [
        ProjectAgentTaskResult {
            status: ProjectAgentTaskStatus::Completed,
            agent_id: agent_id.clone(),
            task_id: "task-1".to_string(),
            result: "Located the navigation route.".to_string(),
            artifacts: Vec::new(),
            evidence: vec!["AGENTS_NAVIGATION.md:241".to_string()],
            memory_candidates: vec!["Use focused navigation.".to_string()],
            improvement_proposals: vec!["Add a narrow search tool.".to_string()],
            error: None,
        },
        ProjectAgentTaskResult {
            status: ProjectAgentTaskStatus::Completed,
            agent_id: agent_id.clone(),
            task_id: "task-2".to_string(),
            result: "Confirmed the same route.".to_string(),
            artifacts: Vec::new(),
            evidence: vec!["AGENTS_NAVIGATION.md:243".to_string()],
            memory_candidates: vec!["  use FOCUSED navigation.  ".to_string()],
            improvement_proposals: vec!["ADD a narrow search tool.".to_string()],
            error: None,
        },
        ProjectAgentTaskResult {
            status: ProjectAgentTaskStatus::Completed,
            agent_id: agent_id.clone(),
            task_id: "task-3".to_string(),
            result: "Produced an unsupported candidate.".to_string(),
            artifacts: Vec::new(),
            evidence: Vec::new(),
            memory_candidates: vec!["This candidate has no evidence.".to_string()],
            improvement_proposals: Vec::new(),
            error: None,
        },
    ];
    for result in &results {
        store
            .persist_result(&file_system, scope, result)
            .await
            .expect("persist result");
    }

    let target = ProjectAgentMaintenanceTarget::Agent(agent_id.clone());
    assert_eq!(
        store
            .maintenance_status(&file_system, scope, &target)
            .await
            .expect("pending status"),
        ProjectAgentMaintenanceStatus {
            catalog_revision: 1,
            agents: vec![ProjectAgentMaintenanceAgentStatus {
                agent_id: agent_id.clone(),
                pending: ProjectAgentPendingCounts {
                    memory_candidates: 3,
                    improvement_proposals: 2,
                },
            }],
        }
    );

    let dry_run = store
        .maintain(
            &file_system,
            scope,
            target.clone(),
            ProjectAgentMaintenanceOptions::dry_run("test-suite"),
        )
        .await
        .expect("dry-run maintenance");
    assert_eq!(dry_run.reports.len(), 1);
    let dry_run_report = &dry_run.reports[0];
    assert_eq!(dry_run_report.accepted_count(), 2);
    assert_eq!(dry_run_report.rejected_count(), 3);
    assert_eq!(dry_run_report.pending_after, dry_run_report.pending_before);
    assert_eq!(dry_run.status.catalog_revision, 1);
    assert_eq!(dry_run.status.pending().total(), 5);

    let applied = store
        .maintain(
            &file_system,
            scope,
            target.clone(),
            ProjectAgentMaintenanceOptions::apply("test-suite"),
        )
        .await
        .expect("apply maintenance");
    assert_eq!(applied.reports.len(), 1);
    let report = &applied.reports[0];
    assert_eq!(report.accepted_count(), 2);
    assert_eq!(report.rejected_count(), 3);
    assert_eq!(report.catalog_revision_before, 1);
    assert_eq!(report.catalog_revision_after, 2);
    assert_eq!(report.pending_after, ProjectAgentPendingCounts::default());
    assert_eq!(
        applied.status.pending(),
        ProjectAgentPendingCounts::default()
    );
    assert_eq!(applied.status.catalog_revision, 2);

    let accepted_memory = report
        .decisions
        .iter()
        .find(|decision| {
            decision.kind == ProjectAgentMaintenanceItemKind::MemoryCandidate
                && decision.disposition == ProjectAgentMaintenanceDisposition::Accepted
        })
        .expect("accepted memory decision");
    assert_eq!(accepted_memory.actor, "test-suite");
    assert_eq!(
        accepted_memory.evidence,
        vec!["AGENTS_NAVIGATION.md:241".to_string()]
    );
    let accepted_path = accepted_memory
        .accepted_path
        .as_ref()
        .expect("accepted memory path");
    assert!(
        project_root
            .join(format!("AGENT/agents/query/{accepted_path}"))
            .as_path()
            .is_file()
    );
    assert!(
        project_root
            .join(format!(
                "AGENT/agents/query/maintenance/history/{}.json",
                report.maintenance_id
            ))
            .as_path()
            .is_file()
    );
    for result in &results {
        assert!(
            project_root
                .join(format!(
                    "AGENT/agents/query/tasks/history/{}.json",
                    result.task_id
                ))
                .as_path()
                .is_file()
        );
    }

    let runtime = store
        .load_runtime(&file_system, scope, &agent_id)
        .await
        .expect("reload accepted memory");
    assert_eq!(
        runtime.accepted_memory,
        vec!["Use focused navigation.".to_string()]
    );
    assert_eq!(
        store
            .maintenance_status(&file_system, scope, &target)
            .await
            .expect("settled status"),
        applied.status
    );
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
        revision: 0,
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
