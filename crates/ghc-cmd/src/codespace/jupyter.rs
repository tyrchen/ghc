//! `ghc codespace jupyter` command.

use anyhow::Result;
use clap::Args;
use ghc_core::ios_eprintln;

/// Open a codespace in JupyterLab.
#[derive(Debug, Args)]
pub struct JupyterArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,
}

impl JupyterArgs {
    /// Run the codespace jupyter command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be opened.
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let url = format!("https://github.com/codespaces/{codespace_name}?editor=jupyter",);
        factory.browser().open(&url)?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Opening codespace {} in JupyterLab",
            cs.success_icon(),
            cs.bold(codespace_name),
        );

        Ok(())
    }
}
