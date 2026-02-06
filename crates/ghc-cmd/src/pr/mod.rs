//! Pull request commands (`ghc pr`).

pub mod checkout;
pub mod checks;
pub mod close;
pub mod comment;
pub mod create;
pub mod diff;
pub mod edit;
pub mod list;
pub mod merge;
pub mod ready;
pub mod reopen;
pub mod revert;
pub mod review;
pub mod status;
pub mod update_branch;
pub mod view;

use clap::Subcommand;

/// Manage pull requests.
#[derive(Debug, Subcommand)]
pub enum PrCommand {
    /// Checkout a pull request branch locally.
    Checkout(checkout::CheckoutArgs),
    /// View CI status checks for a pull request.
    Checks(checks::ChecksArgs),
    /// Close a pull request.
    Close(close::CloseArgs),
    /// Add a comment to a pull request.
    Comment(comment::CommentArgs),
    /// Create a pull request.
    Create(create::CreateArgs),
    /// View the diff of a pull request.
    Diff(diff::DiffArgs),
    /// Edit a pull request.
    Edit(edit::EditArgs),
    /// List pull requests in a repository.
    List(list::ListArgs),
    /// Merge a pull request.
    Merge(merge::MergeArgs),
    /// Mark a draft pull request as ready for review.
    Ready(ready::ReadyArgs),
    /// Reopen a closed pull request.
    Reopen(reopen::ReopenArgs),
    /// Revert a merged pull request.
    Revert(revert::RevertArgs),
    /// Add a review to a pull request.
    Review(review::ReviewArgs),
    /// Show the status of pull requests relevant to you.
    Status(status::StatusArgs),
    /// Update the branch of a pull request.
    #[command(name = "update-branch")]
    UpdateBranch(update_branch::UpdateBranchArgs),
    /// View a pull request.
    View(view::ViewArgs),
}

impl PrCommand {
    /// Run the subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Checkout(args) => args.run(factory).await,
            Self::Checks(args) => args.run(factory).await,
            Self::Close(args) => args.run(factory).await,
            Self::Comment(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Diff(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Merge(args) => args.run(factory).await,
            Self::Ready(args) => args.run(factory).await,
            Self::Reopen(args) => args.run(factory).await,
            Self::Revert(args) => args.run(factory).await,
            Self::Review(args) => args.run(factory).await,
            Self::Status(args) => args.run(factory).await,
            Self::UpdateBranch(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
