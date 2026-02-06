//! Label commands (`ghc label`).
//!
//! Manage labels in a GitHub repository.

pub mod clone;
pub mod create;
pub mod delete;
pub mod edit;
pub mod list;

use clap::Subcommand;

/// Manage labels.
#[derive(Debug, Subcommand)]
pub enum LabelCommand {
    /// Clone labels from another repository.
    Clone(clone::CloneArgs),
    /// Create a label.
    Create(create::CreateArgs),
    /// Delete a label.
    Delete(delete::DeleteArgs),
    /// Edit a label.
    Edit(edit::EditArgs),
    /// List labels.
    #[command(alias = "ls")]
    List(list::ListArgs),
}

impl LabelCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Clone(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
        }
    }
}
