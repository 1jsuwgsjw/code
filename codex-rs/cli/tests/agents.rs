use anyhow::Result;
use codex_project_agents::PROJECT_AGENT_SCHEMA_VERSION;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentRegistration;
use codex_project_agents::ProjectAgentRegistry;
use codex_project_agents::RelativeProjectAgentPath;
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
            "tools": [],
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

    Ok(())
}
