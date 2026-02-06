//! Repository commands (`ghc repo`).

pub mod archive;
pub mod autolink;
pub mod clone;
pub mod create;
pub mod delete;
pub mod deploy_key;
pub mod edit;
pub mod fork;
pub mod gitignore;
pub mod license;
pub mod list;
pub mod rename;
pub mod set_default;
pub mod sync;
pub mod unarchive;
pub mod view;

use clap::Subcommand;

/// Manage repositories.
#[derive(Debug, Subcommand)]
pub enum RepoCommand {
    /// Archive a repository.
    Archive(archive::ArchiveArgs),
    /// Manage autolink references in a repository.
    #[command(subcommand)]
    Autolink(autolink::AutolinkCommand),
    /// Clone a repository locally.
    Clone(clone::CloneArgs),
    /// Create a new repository.
    Create(create::CreateArgs),
    /// Delete a repository.
    Delete(delete::DeleteArgs),
    /// Manage deploy keys in a repository.
    #[command(subcommand, name = "deploy-key")]
    DeployKey(deploy_key::DeployKeyCommand),
    /// Edit repository settings.
    Edit(edit::EditArgs),
    /// Create a fork of a repository.
    Fork(fork::ForkArgs),
    /// List and view available repository gitignore templates.
    #[command(subcommand)]
    Gitignore(gitignore::GitignoreCommand),
    /// Explore repository licenses.
    #[command(subcommand)]
    License(license::LicenseCommand),
    /// List repositories owned by user or organization.
    List(list::ListArgs),
    /// Rename a repository.
    Rename(rename::RenameArgs),
    /// Configure default repository for this directory.
    #[command(name = "set-default")]
    SetDefault(set_default::SetDefaultArgs),
    /// Sync a repository.
    Sync(sync::SyncArgs),
    /// Unarchive a repository.
    Unarchive(unarchive::UnarchiveArgs),
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
            Self::Archive(args) => args.run(factory).await,
            Self::Autolink(cmd) => cmd.run(factory).await,
            Self::Clone(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::DeployKey(cmd) => cmd.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::Fork(args) => args.run(factory).await,
            Self::Gitignore(cmd) => cmd.run(factory).await,
            Self::License(cmd) => cmd.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Rename(args) => args.run(factory).await,
            Self::SetDefault(args) => args.run(factory).await,
            Self::Sync(args) => args.run(factory).await,
            Self::Unarchive(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
