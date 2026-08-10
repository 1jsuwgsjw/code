//! Runtime integration for project-local specialist AGENT definitions.

mod delegate;
mod state;
mod x_tools;

pub use state::install;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
