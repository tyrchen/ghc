//! Ruleset commands (`ghc ruleset`).
//!
//! View and manage repository rulesets.

pub mod check;
pub mod list;
pub mod view;

use clap::Subcommand;

/// Manage repository rulesets.
#[derive(Debug, Subcommand)]
pub enum RulesetCommand {
    /// Check rules that apply to a branch.
    Check(check::CheckArgs),
    /// List rulesets for a repository.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// View a ruleset.
    View(view::ViewArgs),
}

impl RulesetCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Check(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
