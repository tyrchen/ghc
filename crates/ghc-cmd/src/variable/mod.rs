//! Variable commands (`ghc variable`).
//!
//! Manage GitHub Actions variables for a repository.

pub mod delete;
pub mod get;
pub mod list;
pub mod set;

use clap::Subcommand;

/// Manage GitHub Actions variables.
#[derive(Debug, Subcommand)]
pub enum VariableCommand {
    /// Delete a variable.
    Delete(delete::DeleteArgs),
    /// Get a variable value.
    Get(get::GetArgs),
    /// List variables.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Set a variable.
    Set(set::SetArgs),
}

impl VariableCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Delete(args) => args.run(factory).await,
            Self::Get(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Set(args) => args.run(factory).await,
        }
    }
}
