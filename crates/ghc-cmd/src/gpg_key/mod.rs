//! GPG key commands (`ghc gpg-key`).
//!
//! Manage GPG keys for the authenticated GitHub user.

pub mod add;
pub mod delete;
pub mod list;

use clap::Subcommand;

/// Manage GPG keys.
#[derive(Debug, Subcommand)]
pub enum GpgKeyCommand {
    /// Add a GPG key to your GitHub account.
    Add(add::AddArgs),
    /// Delete a GPG key from your GitHub account.
    Delete(delete::DeleteArgs),
    /// List GPG keys on your GitHub account.
    #[command(alias = "ls")]
    List(list::ListArgs),
}

impl GpgKeyCommand {
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
