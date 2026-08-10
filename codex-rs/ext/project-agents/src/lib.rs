//! Runtime integration for project-local specialist AGENT definitions.

mod delegate;
mod events;
mod maintenance;
mod state;
mod x_tools;

pub use maintenance::ThreadProjectAgentMaintenanceOutcome;
pub use maintenance::maintain_thread_project_agents;
pub use state::install;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
