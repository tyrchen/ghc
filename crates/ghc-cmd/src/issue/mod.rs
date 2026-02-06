//! Issue commands (`ghc issue`).

pub mod close;
pub mod comment;
pub mod create;
pub mod delete;
pub mod develop;
pub mod edit;
pub mod list;
pub mod lock;
pub mod pin;
pub mod reopen;
pub mod status;
pub mod transfer;
pub mod unpin;
pub mod view;

use clap::Subcommand;

/// Manage issues.
#[derive(Debug, Subcommand)]
pub enum IssueCommand {
    /// Close an issue.
    Close(close::CloseArgs),
    /// Comment on an issue.
    Comment(comment::CommentArgs),
    /// Create a new issue.
    Create(create::CreateArgs),
    /// Delete an issue.
    Delete(delete::DeleteArgs),
    /// Create a branch for an issue.
    Develop(develop::DevelopArgs),
    /// Edit an issue.
    Edit(edit::EditArgs),
    /// List issues in a repository.
    List(list::ListArgs),
    /// Lock an issue.
    Lock(lock::LockArgs),
    /// Pin an issue.
    Pin(pin::PinArgs),
    /// Reopen a closed issue.
    Reopen(reopen::ReopenArgs),
    /// Show status of relevant issues.
    Status(status::StatusArgs),
    /// Transfer an issue to another repository.
    Transfer(transfer::TransferArgs),
    /// Unpin an issue.
    Unpin(unpin::UnpinArgs),
    /// View an issue.
    View(view::ViewArgs),
}

impl IssueCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Close(args) => args.run(factory).await,
            Self::Comment(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Develop(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Lock(args) => args.run(factory).await,
            Self::Pin(args) => args.run(factory).await,
            Self::Reopen(args) => args.run(factory).await,
            Self::Status(args) => args.run(factory).await,
            Self::Transfer(args) => args.run(factory).await,
            Self::Unpin(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
