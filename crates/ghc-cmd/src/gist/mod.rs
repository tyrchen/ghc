//! Gist commands (`ghc gist`).
//!
//! Manage GitHub gists from the command line.

pub mod clone;
pub mod create;
pub mod delete;
pub mod edit;
pub mod list;
pub mod rename;
pub mod view;

use clap::Subcommand;

/// Manage gists.
#[derive(Debug, Subcommand)]
pub enum GistCommand {
    /// Clone a gist locally.
    Clone(clone::CloneArgs),
    /// Create a new gist.
    Create(create::CreateArgs),
    /// Delete a gist.
    Delete(delete::DeleteArgs),
    /// Edit a gist.
    Edit(edit::EditArgs),
    /// List your gists.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Rename a file in a gist.
    Rename(rename::RenameArgs),
    /// View a gist.
    View(view::ViewArgs),
}

impl GistCommand {
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
            Self::Rename(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
