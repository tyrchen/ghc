//! Repository commands (`ghc repo`).

pub mod clone;
pub mod create;
pub mod list;
pub mod view;

use clap::Subcommand;

/// Manage repositories.
#[derive(Debug, Subcommand)]
pub enum RepoCommand {
    /// Clone a repository locally.
    Clone(clone::CloneArgs),
    /// Create a new repository.
    Create(create::CreateArgs),
    /// List repositories owned by user or organization.
    List(list::ListArgs),
    /// View a repository.
    View(view::ViewArgs),
}

impl RepoCommand {
    /// Run the subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Clone(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
