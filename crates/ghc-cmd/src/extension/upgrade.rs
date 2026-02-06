//! `ghc extension upgrade` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Upgrade installed extensions.
#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Extension name to upgrade (with or without `gh-` prefix).
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Upgrade all installed extensions.
    #[arg(long)]
    all: bool,

    /// Force upgrade even if already up-to-date.
    #[arg(long)]
    force: bool,

    /// Run in dry-run mode (show what would be done).
    #[arg(long)]
    dry_run: bool,
}

impl UpgradeArgs {
    /// Run the extension upgrade command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension cannot be upgraded.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let extensions_dir = ghc_core::config::config_dir().join("extensions");

        if !extensions_dir.exists() {
            ios_eprintln!(ios, "No extensions installed");
            return Ok(());
        }

        // Validate: either a name or --all, not both, and at least one
        if self.name.is_some() && self.all {
            return Err(anyhow::anyhow!("cannot use both extension name and --all"));
        }
        if self.name.is_none() && !self.all {
            return Err(anyhow::anyhow!(
                "specify an extension name or use --all to upgrade all"
            ));
        }

        if let Some(name) = &self.name {
            let ext_name = if name.starts_with("gh-") {
                name.clone()
            } else {
                format!("gh-{name}")
            };

            let ext_dir = extensions_dir.join(&ext_name);
            if !ext_dir.exists() {
                return Err(anyhow::anyhow!("extension {ext_name} is not installed"));
            }

            self.upgrade_extension(ios, &ext_dir, &ext_name, &cs)
                .await?;
        } else {
            // Upgrade all extensions
            let mut entries = tokio::fs::read_dir(&extensions_dir)
                .await
                .context("failed to read extensions directory")?;

            let mut upgraded = 0u32;
            let mut failed = 0u32;

            while let Some(entry) = entries
                .next_entry()
                .await
                .context("failed to read directory entry")?
            {
                let name = entry.file_name();
                let name = name.to_string_lossy().to_string();

                if !name.starts_with("gh-") {
                    continue;
                }

                let metadata = entry
                    .metadata()
                    .await
                    .context("failed to read entry metadata")?;
                if !metadata.is_dir() {
                    continue;
                }

                match self.upgrade_extension(ios, &entry.path(), &name, &cs).await {
                    Ok(()) => upgraded += 1,
                    Err(e) => {
                        ios_eprintln!(ios, "{} Failed to upgrade {name}: {e}", cs.error("X"));
                        failed += 1;
                    }
                }
            }

            if upgraded == 0 && failed == 0 {
                ios_eprintln!(ios, "No extensions to upgrade");
            } else if failed > 0 {
                ios_eprintln!(ios, "{upgraded} upgraded, {failed} failed");
            }
        }

        Ok(())
    }

    /// Upgrade a single extension by pulling the latest changes.
    async fn upgrade_extension(
        &self,
        ios: &ghc_core::iostreams::IOStreams,
        path: &std::path::Path,
        name: &str,
        cs: &ghc_core::iostreams::ColorScheme,
    ) -> Result<()> {
        // Check if this is a binary extension (no .git directory)
        let is_git = path.join(".git").exists();

        if self.dry_run {
            if is_git {
                ios_eprintln!(ios, "[dry-run] Would upgrade {}", cs.bold(name));
            } else {
                ios_eprintln!(
                    ios,
                    "[dry-run] Would upgrade {} (binary, requires reinstall)",
                    cs.bold(name)
                );
            }
            return Ok(());
        }

        if !is_git {
            ios_eprintln!(
                ios,
                "{} {} is a binary extension; use `ghc ext install --force OWNER/REPO` to upgrade",
                cs.warning("!"),
                name
            );
            return Ok(());
        }

        // Get current HEAD before pull
        let before = tokio::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .await
            .context("failed to get current HEAD")?;
        let before_sha = String::from_utf8_lossy(&before.stdout).trim().to_string();

        let mut args = vec!["pull".to_string(), "--ff-only".to_string()];
        if self.force {
            args = vec!["pull".to_string(), "--rebase".to_string()];
        }

        let output = tokio::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output()
            .await
            .with_context(|| format!("failed to run git pull for {name}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("failed to upgrade {name}: {stderr}"));
        }

        // Get HEAD after pull
        let after = tokio::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .await
            .context("failed to get new HEAD")?;
        let after_sha = String::from_utf8_lossy(&after.stdout).trim().to_string();

        if before_sha == after_sha && !self.force {
            ios_eprintln!(ios, "{} {} already up to date", cs.success_icon(), name);
        } else {
            ios_eprintln!(ios, "{} Upgraded {}", cs.success_icon(), cs.bold(name));
        }

        Ok(())
    }
}
