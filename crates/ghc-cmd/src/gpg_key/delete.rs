//! `ghc gpg-key delete` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Delete a GPG key from your GitHub account.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The ID of the GPG key to delete.
    #[arg(value_name = "KEY_ID")]
    id: u64,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl DeleteArgs {
    /// Run the gpg-key delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let path = format!("user/gpg_keys/{}", self.id);

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete GPG key")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted GPG key (ID: {})",
            cs.success_icon(),
            cs.bold(&self.id.to_string()),
        );

        Ok(())
    }
}
