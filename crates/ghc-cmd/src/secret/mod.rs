//! Secret commands (`ghc secret`).
//!
//! Manage GitHub Actions secrets for a repository.

pub mod delete;
pub mod list;
pub mod set;

use clap::Subcommand;

/// Manage GitHub Actions secrets.
#[derive(Debug, Subcommand)]
pub enum SecretCommand {
    /// Delete a secret.
    Delete(delete::DeleteArgs),
    /// List secrets.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Set a secret.
    Set(set::SetArgs),
}

impl SecretCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Delete(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Set(args) => args.run(factory).await,
        }
    }
}
