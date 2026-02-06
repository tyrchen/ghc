//! Cache commands (`ghc cache`).
//!
//! Manage GitHub Actions caches for a repository.

pub mod delete;
pub mod list;

use clap::Subcommand;

/// Manage GitHub Actions caches.
#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Delete a cache entry.
    Delete(delete::DeleteArgs),
    /// List cache entries.
    #[command(alias = "ls")]
    List(list::ListArgs),
}

impl CacheCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Delete(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
        }
    }
}
