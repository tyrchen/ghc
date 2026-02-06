//! Command implementations for the GHC GitHub CLI.
//!
//! Each module corresponds to a `gh` top-level command group.

pub mod accessibility;
pub mod actions;
pub mod agent_task;
pub mod alias;
pub mod api;
pub mod attestation;
pub mod auth;
pub mod browse;
pub mod cache;
pub mod codespace;
pub mod completion;
pub mod config;
pub mod copilot;
pub mod extension;
pub mod factory;
pub mod gist;
pub mod gpg_key;
pub mod issue;
pub mod label;
pub mod org;
pub mod pr;
pub mod preview;
pub mod project;
pub mod release;
pub mod repo;
pub mod ruleset;
pub mod run;
pub mod search;
pub mod secret;
pub mod ssh_key;
pub mod status;
pub mod variable;
pub mod version;
pub mod workflow;

#[cfg(test)]
pub mod test_helpers;
