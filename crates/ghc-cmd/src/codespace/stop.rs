//! `ghc codespace stop` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Stop a running codespace.
#[derive(Debug, Args)]
pub struct StopArgs {
    /// Name of the codespace to stop.
    #[arg(short, long)]
    codespace: Option<String>,
}

impl StopArgs {
    /// Run the codespace stop command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be stopped.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let client = factory.api_client("github.com")?;

        let path = format!("user/codespaces/{codespace_name}/stop");

        client
            .rest_text(reqwest::Method::POST, &path, None)
            .await
            .context("failed to stop codespace")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Stopped codespace {}",
            cs.success_icon(),
            cs.bold(codespace_name),
        );

        Ok(())
    }
}
