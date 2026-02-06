//! `ghc extension upgrade` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Upgrade installed extensions.
#[derive(Debug, Args)]
pub struct UpgradeArgs {
    /// Extension name to upgrade (with or without `gh-` prefix). If omitted, upgrades all.
    #[arg(value_name = "NAME")]
    name: Option<String>,

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

                self.upgrade_extension(ios, &entry.path(), &name, &cs)
                    .await?;
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
        if self.dry_run {
            ios_eprintln!(ios, "[dry-run] Would upgrade {}", cs.bold(name));
            return Ok(());
        }

        let mut args = vec!["pull".to_string()];
        if self.force {
            args.push("--force".to_string());
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

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("Already up to date") && !self.force {
            ios_eprintln!(ios, "{} {} already up to date", cs.success_icon(), name);
        } else {
            ios_eprintln!(ios, "{} Upgraded {}", cs.success_icon(), cs.bold(name));
        }

        Ok(())
    }
}
