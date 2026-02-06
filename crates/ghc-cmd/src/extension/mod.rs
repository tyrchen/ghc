//! Extension commands (`ghc extension`).
//!
//! Manage gh extensions.

pub mod browse;
pub mod create;
pub mod install;
pub mod list;
pub mod remove;
pub mod search;
pub mod upgrade;

use anyhow::Result;
use clap::Subcommand;

/// Manage GitHub CLI extensions.
#[derive(Debug, Subcommand)]
pub enum ExtensionCommand {
    /// Browse popular extensions.
    Browse(browse::BrowseArgs),
    /// Create a new extension.
    Create(create::CreateArgs),
    /// Install an extension from a repository.
    Install(install::InstallArgs),
    /// List installed extensions.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Remove an installed extension.
    Remove(remove::RemoveArgs),
    /// Search for extensions.
    Search(search::SearchArgs),
    /// Upgrade installed extensions.
    Upgrade(upgrade::UpgradeArgs),
}

impl ExtensionCommand {
    /// Run the extension subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        match self {
            Self::Browse(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Install(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Remove(args) => args.run(factory).await,
            Self::Search(args) => args.run(factory).await,
            Self::Upgrade(args) => args.run(factory).await,
        }
    }
}
