//! `ghc codespace ssh` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// SSH into a codespace.
#[derive(Debug, Args)]
pub struct SshArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Run a command via SSH instead of opening a shell.
    #[arg(last = true)]
    command: Vec<String>,

    /// Enable debug output for the SSH connection.
    #[arg(short, long)]
    debug: bool,

    /// Path to the SSH config file.
    #[arg(long)]
    config: Option<String>,
}

impl SshArgs {
    /// Run the codespace ssh command.
    ///
    /// # Errors
    ///
    /// Returns an error if the SSH connection fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "Connecting to codespace {} via SSH...",
            cs.bold(codespace_name),
        );

        let mut ssh_args = vec!["ssh"];

        if self.debug {
            ssh_args.push("-v");
        }

        if let Some(ref config) = self.config {
            ssh_args.push("-F");
            ssh_args.push(config);
        }

        // The actual SSH target for codespaces uses the gh CLI's ssh proxy.
        // For now, we use the codespace name as the host identifier.
        let host = format!("codespace-{codespace_name}");
        ssh_args.push(&host);

        // Add remote command if specified
        let cmd_str = self.command.join(" ");
        if !cmd_str.is_empty() {
            ssh_args.push(&cmd_str);
        }

        let status = tokio::process::Command::new("ssh")
            .args(&ssh_args[1..])
            .status()
            .await
            .context("failed to execute ssh")?;

        if !status.success() {
            anyhow::bail!("ssh exited with code {}", status.code().unwrap_or(1),);
        }

        Ok(())
    }
}
