use anyhow::Context;
use clap::Args;
use clap::Parser;
use codex_exec_server::LOCAL_FS;
use codex_project_agents::ProjectAgentEntry;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentId;
use codex_project_agents::ProjectAgentMaintenanceOptions;
use codex_project_agents::ProjectAgentMaintenanceTarget;
use codex_project_agents::ProjectAgentStore;
use codex_project_agents::RelativeProjectAgentPath;
use codex_project_agents::resolve_project_root;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
pub struct AgentCli {
    #[command(subcommand)]
    subcommand: AgentSubcommand,
}

#[derive(Debug, clap::Subcommand)]
enum AgentSubcommand {
    /// List registered project AGENT definitions.
    List(AgentListArgs),
    /// Show one registered project AGENT definition.
    Show(AgentShowArgs),
    /// Create and register a project AGENT definition.
    Create(AgentCreateArgs),
    /// Disable a registered project AGENT definition.
    Disable(AgentIdArgs),
    /// Review pending memory candidates and improvement proposals.
    Maintain(AgentMaintainArgs),
}

#[derive(Debug, Args)]
struct AgentListArgs {
    /// Output the registered AGENT definitions as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AgentShowArgs {
    /// Registered AGENT identifier.
    id: ProjectAgentId,

    /// Output the AGENT definition as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AgentCreateArgs {
    /// New AGENT identifier.
    id: ProjectAgentId,

    /// Description used by the main AGENT to select this specialist.
    #[arg(long)]
    description: String,
}

#[derive(Debug, Args)]
struct AgentIdArgs {
    /// Registered AGENT identifier.
    id: ProjectAgentId,
}

#[derive(Debug, Args)]
struct AgentMaintainArgs {
    /// Restrict maintenance to one registered AGENT.
    id: Option<ProjectAgentId>,

    /// Report decisions without changing accepted memory or proposal state.
    #[arg(long)]
    dry_run: bool,
}

impl AgentCli {
    pub async fn run(self, cwd: Option<PathBuf>) -> anyhow::Result<()> {
        let cwd = match cwd {
            Some(cwd) => AbsolutePathBuf::relative_to_current_dir(cwd)?,
            None => AbsolutePathBuf::current_dir()?,
        };
        let cwd = PathUri::from_abs_path(&cwd);
        let markers = [
            RelativeProjectAgentPath::new("AGENT")?,
            RelativeProjectAgentPath::new(".git")?,
        ];
        let project_root = resolve_project_root(
            LOCAL_FS.as_ref(),
            &cwd,
            &markers,
            ProjectAgentFileSystemScope::Unrestricted,
        )
        .await
        .context("failed to resolve the project root for project AGENTs")?;
        let store = ProjectAgentStore::new(project_root)?;
        store
            .bootstrap(LOCAL_FS.as_ref(), ProjectAgentFileSystemScope::Unrestricted)
            .await?;

        match self.subcommand {
            AgentSubcommand::List(args) => {
                let entries = store
                    .list(LOCAL_FS.as_ref(), ProjectAgentFileSystemScope::Unrestricted)
                    .await?;
                if args.json {
                    let entries = entries
                        .into_iter()
                        .map(JsonAgentEntry::from)
                        .collect::<Vec<_>>();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                } else if entries.is_empty() {
                    println!(
                        "No project AGENT definitions found in `{}`.",
                        store.project_root()
                    );
                } else {
                    println!("ID\tSTATUS\tDESCRIPTION");
                    for entry in entries {
                        let status = if entry.enabled { "enabled" } else { "disabled" };
                        println!(
                            "{}\t{}\t{}",
                            entry.definition.id, status, entry.definition.description
                        );
                    }
                }
            }
            AgentSubcommand::Show(AgentShowArgs { id, json }) => {
                let entry = store
                    .get(
                        LOCAL_FS.as_ref(),
                        ProjectAgentFileSystemScope::Unrestricted,
                        &id,
                    )
                    .await?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&JsonAgentEntry::from(entry))?
                    );
                } else {
                    print_agent(&entry);
                }
            }
            AgentSubcommand::Create(AgentCreateArgs { id, description }) => {
                let entry = store
                    .create(
                        LOCAL_FS.as_ref(),
                        ProjectAgentFileSystemScope::Unrestricted,
                        id,
                        description,
                    )
                    .await?;
                println!(
                    "Created project AGENT `{}` at `AGENT/{}`.",
                    entry.definition.id, entry.path
                );
            }
            AgentSubcommand::Disable(AgentIdArgs { id }) => {
                let entry = store
                    .disable(
                        LOCAL_FS.as_ref(),
                        ProjectAgentFileSystemScope::Unrestricted,
                        &id,
                    )
                    .await?;
                println!("Disabled project AGENT `{}`.", entry.definition.id);
            }
            AgentSubcommand::Maintain(AgentMaintainArgs { id, dry_run }) => {
                let target = id.map_or(
                    ProjectAgentMaintenanceTarget::All,
                    ProjectAgentMaintenanceTarget::Agent,
                );
                let actor = std::env::var("USERNAME")
                    .or_else(|_| std::env::var("USER"))
                    .map(|user| format!("codex-cli:{user}"))
                    .unwrap_or_else(|_| "codex-cli".to_string());
                let options = if dry_run {
                    ProjectAgentMaintenanceOptions::dry_run(actor)
                } else {
                    ProjectAgentMaintenanceOptions::apply(actor)
                };
                let outcome = store
                    .maintain(
                        LOCAL_FS.as_ref(),
                        ProjectAgentFileSystemScope::Unrestricted,
                        target,
                        options,
                    )
                    .await?;
                if outcome.reports.is_empty() {
                    println!("No registered project AGENT definitions found.");
                }
                for report in outcome.reports {
                    let prefix = if dry_run {
                        "Dry-run maintenance"
                    } else {
                        "Maintenance"
                    };
                    println!(
                        "{prefix} `{}` for `{}`: {} accepted, {} rejected; pending {} -> {}; catalog revision {} -> {}.",
                        report.maintenance_id,
                        report.agent_id,
                        report.accepted_count(),
                        report.rejected_count(),
                        report.pending_before.total(),
                        report.pending_after.total(),
                        report.catalog_revision_before,
                        report.catalog_revision_after,
                    );
                    for decision in report.decisions {
                        println!(
                            "  {} {}: {} ({})",
                            decision.disposition.as_str(),
                            decision.kind.as_str(),
                            decision.item_id,
                            decision.reason,
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

fn print_agent(entry: &ProjectAgentEntry) {
    let definition = &entry.definition;
    println!("id: {}", definition.id);
    println!(
        "status: {}",
        if entry.enabled { "enabled" } else { "disabled" }
    );
    println!("description: {}", definition.description);
    println!("definition: AGENT/{}", entry.path);
    println!("constraints: {}", definition.constraints_file);
    println!(
        "model: {}",
        definition.model.as_deref().unwrap_or("inherit")
    );
    println!(
        "reasoning effort: {}",
        definition
            .model_reasoning_effort
            .as_deref()
            .unwrap_or("inherit")
    );
    println!(
        "model provider: {}",
        definition.model_provider.as_deref().unwrap_or("inherit")
    );
    println!("tools: {}", definition.tools.len());
    println!("memory max items: {}", definition.memory_max_items);
    println!("memory max tokens: {}", definition.memory_max_tokens);
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonAgentEntry {
    id: String,
    description: String,
    enabled: bool,
    path: String,
    constraints_file: String,
    model: Option<String>,
    model_reasoning_effort: Option<String>,
    model_provider: Option<String>,
    tools: Vec<String>,
    memory_max_items: u32,
    memory_max_tokens: u32,
}

impl From<ProjectAgentEntry> for JsonAgentEntry {
    fn from(entry: ProjectAgentEntry) -> Self {
        let definition = entry.definition;
        Self {
            id: definition.id.to_string(),
            description: definition.description,
            enabled: entry.enabled,
            path: entry.path.to_string(),
            constraints_file: definition.constraints_file.to_string(),
            model: definition.model,
            model_reasoning_effort: definition.model_reasoning_effort,
            model_provider: definition.model_provider,
            tools: definition
                .tools
                .into_iter()
                .map(|tool| tool.to_string())
                .collect(),
            memory_max_items: definition.memory_max_items,
            memory_max_tokens: definition.memory_max_tokens,
        }
    }
}
