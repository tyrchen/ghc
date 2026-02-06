//! `ghc codespace code` command.

use anyhow::Result;
use clap::Args;
use ghc_core::ios_eprintln;

/// Open a codespace in VS Code.
#[derive(Debug, Args)]
pub struct CodeArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Open in the browser-based editor instead of desktop VS Code.
    #[arg(long)]
    web: bool,

    /// Use VS Code Insiders.
    #[arg(long)]
    insiders: bool,
}

impl CodeArgs {
    /// Run the codespace code command.
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

        if self.web {
            let url = format!("https://github.com/codespaces/{codespace_name}?editor=vscode",);
            factory.browser().open(&url)?;
            return Ok(());
        }

        let scheme = if self.insiders {
            "vscode-insiders"
        } else {
            "vscode"
        };

        let url = format!("{scheme}://github.codespaces/connect?name={codespace_name}",);
        factory.browser().open(&url)?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Opening codespace {} in VS Code",
            cs.success_icon(),
            cs.bold(codespace_name),
        );

        Ok(())
    }
}
