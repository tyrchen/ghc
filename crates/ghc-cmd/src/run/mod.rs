//! Run commands (`ghc run`).
//!
//! Manage GitHub Actions workflow runs.

pub mod cancel;
pub mod delete;
pub mod download;
pub mod list;
pub mod rerun;
pub mod view;
pub mod watch;

use clap::Subcommand;

/// Manage workflow runs.
#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Cancel a workflow run.
    Cancel(cancel::CancelArgs),
    /// Delete a workflow run.
    Delete(delete::DeleteArgs),
    /// Download run artifacts.
    Download(download::DownloadArgs),
    /// List recent workflow runs.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Rerun a workflow run.
    Rerun(rerun::RerunArgs),
    /// View a workflow run.
    View(view::ViewArgs),
    /// Watch a run until it completes.
    Watch(watch::WatchArgs),
}

impl RunCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Cancel(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Download(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Rerun(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
            Self::Watch(args) => args.run(factory).await,
        }
    }
}
