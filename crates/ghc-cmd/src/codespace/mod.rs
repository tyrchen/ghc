//! Codespace commands (`ghc codespace`).
//!
//! Manage GitHub Codespaces.

pub mod code;
pub mod create;
pub mod delete;
pub mod edit;
pub mod jupyter;
pub mod list;
pub mod logs;
pub mod ports;
pub mod rebuild;
pub mod ssh;
pub mod stop;
pub mod view;

use clap::Subcommand;

/// Manage codespaces.
#[derive(Debug, Subcommand)]
pub enum CodespaceCommand {
    /// Open a codespace in VS Code.
    Code(code::CodeArgs),
    /// Create a codespace.
    Create(create::CreateArgs),
    /// Delete a codespace.
    Delete(delete::DeleteArgs),
    /// Edit a codespace.
    Edit(edit::EditArgs),
    /// Open a codespace in JupyterLab.
    Jupyter(jupyter::JupyterArgs),
    /// List codespaces.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// View codespace logs.
    Logs(logs::LogsArgs),
    /// Manage codespace port forwarding.
    Ports(ports::PortsArgs),
    /// Rebuild a codespace.
    Rebuild(rebuild::RebuildArgs),
    /// SSH into a codespace.
    Ssh(ssh::SshArgs),
    /// Stop a running codespace.
    Stop(stop::StopArgs),
    /// View a codespace.
    View(view::ViewArgs),
}

impl CodespaceCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Code(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::Jupyter(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Logs(args) => args.run(factory).await,
            Self::Ports(args) => args.run(factory).await,
            Self::Rebuild(args) => args.run(factory).await,
            Self::Ssh(args) => args.run(factory).await,
            Self::Stop(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
