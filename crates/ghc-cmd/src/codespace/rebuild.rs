//! `ghc codespace rebuild` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Rebuild a codespace.
#[derive(Debug, Args)]
pub struct RebuildArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Perform a full rebuild (not incremental).
    #[arg(long)]
    full: bool,
}

impl RebuildArgs {
    /// Run the codespace rebuild command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be rebuilt.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let client = factory.api_client("github.com")?;

        let path = format!("user/codespaces/{codespace_name}/start");

        client
            .rest_text(reqwest::Method::POST, &path, None)
            .await
            .context("failed to rebuild codespace")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        let rebuild_type = if self.full { "full " } else { "" };
        ios_eprintln!(
            ios,
            "{} Started {rebuild_type}rebuild of codespace {}",
            cs.success_icon(),
            cs.bold(codespace_name),
        );

        Ok(())
    }
}
