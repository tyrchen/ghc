//! Actions command (`ghc actions`).
//!
//! Displays help information about GitHub Actions-related commands
//! including `run`, `workflow`, and `cache`.

use anyhow::Result;
use clap::Args;
use ghc_core::ios_println;

/// Learn about working with GitHub Actions.
#[derive(Debug, Args)]
pub struct ActionsArgs;

impl ActionsArgs {
    /// Run the actions help command.
    ///
    /// # Errors
    ///
    /// This command does not return errors under normal operation.
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        ios_println!(ios, "Work with GitHub Actions");
        ios_println!(ios);
        ios_println!(ios, "USAGE");
        ios_println!(ios, "  ghc <command> <subcommand> [flags]");
        ios_println!(ios);
        ios_println!(ios, "AVAILABLE COMMANDS");
        ios_println!(ios, "  run        View details about workflow runs");
        ios_println!(
            ios,
            "  workflow   View details about GitHub Actions workflows"
        );
        ios_println!(ios, "  cache      Manage GitHub Actions caches");
        ios_println!(ios);
        ios_println!(ios, "LEARN MORE");
        ios_println!(
            ios,
            "  Use 'ghc <command> --help' for more information about a command."
        );
        ios_println!(ios, "  https://docs.github.com/en/actions");

        Ok(())
    }
}
