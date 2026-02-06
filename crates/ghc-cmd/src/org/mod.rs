//! Organization commands (`ghc org`).
//!
//! Manage GitHub organizations.

pub mod list;

use clap::Subcommand;

/// Manage organizations.
#[derive(Debug, Subcommand)]
pub enum OrgCommand {
    /// List organizations for the authenticated user.
    #[command(alias = "ls")]
    List(list::ListArgs),
}

impl OrgCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::List(args) => args.run(factory).await,
        }
    }
}
