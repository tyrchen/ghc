//! `ghc extension remove` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Remove an installed extension.
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Extension name to remove (with or without `gh-` prefix).
    #[arg(value_name = "NAME")]
    name: String,
}

impl RemoveArgs {
    /// Run the extension remove command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension cannot be removed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ext_name = if self.name.starts_with("gh-") {
            self.name.clone()
        } else {
            format!("gh-{}", self.name)
        };

        let ext_dir = ghc_core::config::config_dir()
            .join("extensions")
            .join(&ext_name);

        if !ext_dir.exists() {
            return Err(anyhow::anyhow!("extension {ext_name} is not installed"));
        }

        tokio::fs::remove_dir_all(&ext_dir)
            .await
            .context("failed to remove extension directory")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Removed extension {}",
            cs.success_icon(),
            cs.bold(&ext_name),
        );

        Ok(())
    }
}
