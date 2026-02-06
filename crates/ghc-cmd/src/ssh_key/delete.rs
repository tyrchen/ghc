//! `ghc ssh-key delete` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Delete an SSH key from your GitHub account.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The ID of the SSH key to delete.
    #[arg(value_name = "KEY_ID")]
    id: u64,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl DeleteArgs {
    /// Run the ssh-key delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let path = format!("user/keys/{}", self.id);

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete SSH key")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted SSH key (ID: {})",
            cs.success_icon(),
            cs.bold(&self.id.to_string()),
        );

        Ok(())
    }
}
