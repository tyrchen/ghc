//! SSH key commands (`ghc ssh-key`).
//!
//! Manage SSH keys for the authenticated GitHub user.

pub mod add;
pub mod delete;
pub mod list;

use clap::Subcommand;

/// Manage SSH keys.
#[derive(Debug, Subcommand)]
pub enum SshKeyCommand {
    /// Add an SSH key to your GitHub account.
    Add(add::AddArgs),
    /// Delete an SSH key from your GitHub account.
    Delete(delete::DeleteArgs),
    /// List SSH keys on your GitHub account.
    #[command(alias = "ls")]
    List(list::ListArgs),
}

impl SshKeyCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Add(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
        }
    }
}
