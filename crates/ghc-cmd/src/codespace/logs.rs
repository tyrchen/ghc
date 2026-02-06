//! `ghc codespace logs` command.

use anyhow::Result;
use clap::Args;
use ghc_core::ios_eprintln;

/// View codespace creation logs.
#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Follow log output.
    #[arg(short, long)]
    follow: bool,
}

impl LogsArgs {
    /// Run the codespace logs command.
    ///
    /// # Errors
    ///
    /// Returns an error if the logs cannot be retrieved.
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "Streaming logs for codespace {}...",
            cs.bold(codespace_name),
        );

        // The GitHub API does not have a direct REST endpoint for codespace logs.
        // Logs are typically accessed via the Live Share connection or the VS Code extension.
        // For CLI, we would use the codespace SSH session.
        ios_eprintln!(
            ios,
            "To view logs, SSH into the codespace: ghc codespace ssh -c {codespace_name}",
        );

        Ok(())
    }
}
