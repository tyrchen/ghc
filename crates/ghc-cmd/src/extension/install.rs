//! `ghc extension install` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;

/// Install an extension from a repository.
#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Repository to install (OWNER/REPO or URL).
    #[arg(value_name = "REPO")]
    repo: String,

    /// Pin to a specific release tag or commit.
    #[arg(long)]
    pin: Option<String>,
}

impl InstallArgs {
    /// Run the extension install command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension cannot be installed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let extensions_dir = ghc_core::config::config_dir().join("extensions");

        tokio::fs::create_dir_all(&extensions_dir)
            .await
            .context("failed to create extensions directory")?;

        // Determine repository full name
        let repo_full = if self.repo.contains("://") {
            // Extract owner/repo from URL
            let url = &self.repo;
            let parts: Vec<&str> = url.trim_end_matches('/').rsplitn(3, '/').collect();
            if parts.len() < 2 {
                return Err(anyhow::anyhow!("invalid repository URL: {url}"));
            }
            format!("{}/{}", parts[1], parts[0])
        } else {
            self.repo.clone()
        };

        let repo_name = repo_full
            .split('/')
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("invalid repository format: {repo_full}"))?;

        if !repo_name.starts_with("gh-") {
            return Err(anyhow::anyhow!(
                "extension repository name must start with 'gh-': {repo_name}"
            ));
        }

        let ext_dir = extensions_dir.join(repo_name);
        if ext_dir.exists() {
            return Err(anyhow::anyhow!(
                "extension {repo_name} is already installed",
            ));
        }

        // Clone the repository
        let mut cmd_args = vec![
            "clone".to_string(),
            format!("https://github.com/{repo_full}.git"),
            ext_dir.display().to_string(),
            "--depth=1".to_string(),
        ];

        if let Some(pin) = &self.pin {
            cmd_args.push("--branch".to_string());
            cmd_args.push(pin.clone());
        }

        let status = tokio::process::Command::new("git")
            .args(&cmd_args)
            .status()
            .await
            .context("failed to run git clone")?;

        if !status.success() {
            return Err(anyhow::anyhow!("git clone failed for {repo_full}"));
        }

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Installed extension {}",
            cs.success_icon(),
            cs.bold(repo_name),
        );

        Ok(())
    }
}
