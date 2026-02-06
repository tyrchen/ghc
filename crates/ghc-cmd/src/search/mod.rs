//! Search commands (`ghc search`).
//!
//! Search across GitHub for code, commits, issues, pull requests, and repositories.

pub mod code;
pub mod commits;
pub mod issues;
pub mod prs;
pub mod repos;

use clap::Subcommand;

/// Search across GitHub.
#[derive(Debug, Subcommand)]
pub enum SearchCommand {
    /// Search for code.
    Code(code::CodeArgs),
    /// Search for commits.
    Commits(commits::CommitsArgs),
    /// Search for issues.
    Issues(issues::IssuesArgs),
    /// Search for pull requests.
    Prs(prs::PrsArgs),
    /// Search for repositories.
    Repos(repos::ReposArgs),
}

impl SearchCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Code(args) => args.run(factory).await,
            Self::Commits(args) => args.run(factory).await,
            Self::Issues(args) => args.run(factory).await,
            Self::Prs(args) => args.run(factory).await,
            Self::Repos(args) => args.run(factory).await,
        }
    }
}
