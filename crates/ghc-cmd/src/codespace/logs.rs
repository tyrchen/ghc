//! `ghc codespace logs` command.

use anyhow::{Context, Result};
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

        // Stream creation logs via SSH into the codespace.
        // The log file is at /workspaces/.codespaces/.persistedshare/creation.log
        let log_path = "/workspaces/.codespaces/.persistedshare/creation.log";
        let remote_cmd = if self.follow {
            format!("tail -f {log_path} 2>/dev/null")
        } else {
            format!("cat {log_path} 2>/dev/null")
        };

        let status = tokio::process::Command::new("gh")
            .args(["codespace", "ssh", "-c", codespace_name, "--", &remote_cmd])
            .stdin(std::process::Stdio::null())
            .status()
            .await
            .context("failed to run gh codespace ssh")?;

        if !status.success() {
            return Err(anyhow::anyhow!(
                "failed to retrieve logs from codespace {codespace_name}"
            ));
        }

        Ok(())
    }
}
