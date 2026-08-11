use anyhow::Result;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentRegistration;
use codex_project_agents::ProjectAgentRegistry;
use codex_project_agents::ProjectAgentToolManifest;
use codex_project_agents::ProjectAgentToolTarget;
use codex_project_agents::RelativeProjectAgentPath;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use tempfile::TempDir;

fn codex_command(codex_home: &Path, cwd: &Path) -> Result<assert_cmd::Command> {
    let mut command = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    command.env("CODEX_HOME", codex_home).current_dir(cwd);
    Ok(command)
}

#[test]
fn agents_cli_manages_project_registry() -> Result<()> {
    let codex_home = TempDir::new()?;
    let project = TempDir::new()?;
    std::fs::create_dir(project.path().join(".git"))?;

    codex_command(codex_home.path(), project.path())?
        .args([
            "agents",
            "create",
            "query",
            "--description",
            "Answers bounded project questions.",
        ])
        .assert()
        .success()
        .stdout("Created project AGENT `query` at `AGENT/agents/query/agent.toml`.\n");

    let tools_directory = project.path().join("AGENT/agents/query/tools");
    std::fs::create_dir_all(&tools_directory)?;
    std::fs::write(tools_directory.join("search.py"), "print('ok')\n")?;
    std::fs::write(
        tools_directory.join("search.schema.json"),
        r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}"#,
    )?;
    codex_command(codex_home.path(), project.path())?
        .args([
            "agents",
            "add-command-tool",
            "query",
            "search",
            "--description",
            "Runs bounded project search.",
            "--program",
            "tools/search.py",
            "--input-schema",
            "tools/search.schema.json",
            "--timeout-ms",
            "30000",
        ])
        .assert()
        .success()
        .stdout(
            "Registered command tool `search` for project AGENT `query` at `AGENT/agents/query/tools/search.x`.\n",
        );

    let list_output = codex_command(codex_home.path(), project.path())?
        .args(["agents", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let listed: serde_json::Value = serde_json::from_slice(&list_output)?;
    assert_eq!(
        listed,
        json!([{
            "id": "query",
            "description": "Answers bounded project questions.",
            "enabled": true,
            "path": "agents/query/agent.toml",
            "constraintsFile": "constraints.md",
            "model": null,
            "modelReasoningEffort": null,
            "modelProvider": null,
            "tools": ["tools/search.x"],
            "memoryMaxItems": 32,
            "memoryMaxTokens": 4000
        }])
    );

    let show_output = codex_command(codex_home.path(), project.path())?
        .args(["agents", "show", "query", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let shown: serde_json::Value = serde_json::from_slice(&show_output)?;
    assert_eq!(shown, listed[0]);

    codex_command(codex_home.path(), project.path())?
        .args(["agents", "maintain", "query", "--dry-run"])
        .assert()
        .success()
        .stdout(contains("Dry-run maintenance"))
        .stdout(contains("0 accepted, 0 rejected"));

    codex_command(codex_home.path(), project.path())?
        .args(["agents", "disable", "query"])
        .assert()
        .success()
        .stdout("Disabled project AGENT `query`.\n");

    let registry: ProjectAgentRegistry = toml::from_str(&std::fs::read_to_string(
        project.path().join("AGENT/registry.toml"),
    )?)?;
    assert_eq!(
        registry,
        ProjectAgentRegistry {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            revision: 2,
            agents: BTreeMap::from([(
                ProjectAgentId::new("query")?,
                ProjectAgentRegistration {
                    path: RelativeProjectAgentPath::new("agents/query/agent.toml")?,
                    enabled: false,
                },
            )]),
        }
    );
    assert!(
        project
            .path()
            .join("AGENT/agents/query/constraints.md")
            .is_file()
    );
    let manifest: ProjectAgentToolManifest =
        toml::from_str(&std::fs::read_to_string(tools_directory.join("search.x"))?)?;
    assert_eq!(
        manifest,
        ProjectAgentToolManifest {
            schema_version: PROJECT_AGENT_SCHEMA_VERSION,
            id: ProjectAgentId::new("search")?,
            description: "Runs bounded project search.".to_string(),
            target: ProjectAgentToolTarget::Command {
                program: RelativeProjectAgentPath::new("tools/search.py")?,
            },
            timeout_ms: Some(30_000),
            input_schema: Some(RelativeProjectAgentPath::new("tools/search.schema.json")?),
        }
    );

    Ok(())
}
