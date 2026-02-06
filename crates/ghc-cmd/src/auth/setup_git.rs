//! `ghc auth setup-git` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/setupgit/setupgit.go`.

use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::iostreams::IOStreams;

use crate::factory::Factory;

/// Configure git to use GitHub CLI as a credential helper.
///
/// By default, GitHub CLI will be set as the credential helper for all
/// authenticated hosts. If there are no authenticated hosts, the command
/// fails with an error.
#[derive(Debug, Args)]
pub struct SetupGitArgs {
    /// The hostname to configure git for.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// Force setup even if the host is not known.
    #[arg(short, long)]
    force: bool,
}

impl SetupGitArgs {
    /// Run the setup-git command.
    ///
    /// # Errors
    ///
    /// Returns an error if git configuration fails.
    pub fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;

        if self.hostname.is_none() && self.force {
            anyhow::bail!("`--force` must be used in conjunction with `--hostname`");
        }

        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
        let hostnames = cfg.hosts();

        if let Some(ref hostname) = self.hostname {
            if !self.force && !hostnames.iter().any(|h| h.eq_ignore_ascii_case(hostname)) {
                anyhow::bail!(
                    "You are not logged into the GitHub host \"{hostname}\". Run `ghc auth login -h {hostname}` to authenticate or provide `--force`"
                );
            }

            configure_credential_helper(ios, hostname)?;
        } else {
            if hostnames.is_empty() {
                ios_eprintln!(
                    ios,
                    "You are not logged into any GitHub hosts. Run `ghc auth login` to authenticate."
                );
                anyhow::bail!("");
            }

            for hostname in &hostnames {
                configure_credential_helper(ios, hostname)?;
            }
        }

        Ok(())
    }
}

/// Configure ghc as the git credential helper for a specific host.
///
/// Uses synchronous `std::process::Command` since git config does not require
/// async I/O.
fn configure_credential_helper(ios: &IOStreams, hostname: &str) -> anyhow::Result<()> {
    let helper_pattern = format!("https://{hostname}");
    let helper_cmd = "!ghc auth git-credential";

    // Clear existing credential helper
    let status = std::process::Command::new("git")
        .args([
            "config",
            "--global",
            &format!("credential.{helper_pattern}.helper"),
            "",
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("failed to clear git credential helper for {hostname}");
    }

    // Set ghc as the credential helper
    let status = std::process::Command::new("git")
        .args([
            "config",
            "--global",
            "--add",
            &format!("credential.{helper_pattern}.helper"),
            helper_cmd,
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("failed to set git credential helper for {hostname}");
    }

    ios_eprintln!(ios, "Configured git credential helper for {hostname}");
    Ok(())
}
