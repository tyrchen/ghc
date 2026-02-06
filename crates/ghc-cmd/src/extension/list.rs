//! `ghc extension list` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List installed extensions.
#[derive(Debug, Args)]
pub struct ListArgs {}

impl ListArgs {
    /// Run the extension list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extensions directory cannot be read.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let extensions_dir = ghc_core::config::config_dir().join("extensions");

        if !extensions_dir.exists() {
            ios_eprintln!(ios, "No extensions installed");
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&extensions_dir)
            .await
            .context("failed to read extensions directory")?;
        let mut tp = TablePrinter::new(ios);
        let mut count = 0u32;

        while let Some(entry) = entries
            .next_entry()
            .await
            .context("failed to read directory entry")?
        {
            let name = entry.file_name();
            let name = name.to_string_lossy();

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

            let ext_path = entry.path();
            let is_git = ext_path.join(".git").exists();
            let version = get_extension_version(&ext_path).await;
            let repo_url = if is_git {
                get_remote_url(&ext_path).await
            } else {
                String::new()
            };

            // Check if upgrade is available (for git-based extensions)
            let upgrade_available = if is_git {
                check_upgrade_available(&ext_path).await
            } else {
                false
            };

            let version_display = if upgrade_available {
                format!("{version} {}", cs.warning("Upgrade available"))
            } else {
                version
            };

            tp.add_row(vec![cs.bold(&name), repo_url, version_display]);
            count += 1;
        }

        if count == 0 {
            ios_eprintln!(ios, "No extensions installed");
            return Ok(());
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

/// Get the current version (git tag or commit) of an installed extension.
async fn get_extension_version(path: &std::path::Path) -> String {
    // For binary extensions (no .git dir), look for a manifest or version file
    if !path.join(".git").exists() {
        return String::from("binary");
    }

    let output = tokio::process::Command::new("git")
        .args(["describe", "--tags", "--always"])
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // For non-tagged commits, truncate SHA to 8 chars
            if version.len() == 40 && version.chars().all(|c| c.is_ascii_hexdigit()) {
                version[..8].to_string()
            } else {
                version
            }
        }
        _ => String::from("unknown"),
    }
}

/// Get the remote URL of a git-based extension.
async fn get_remote_url(path: &std::path::Path) -> String {
    let output = tokio::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // Strip .git suffix for display
            url.trim_end_matches(".git").to_string()
        }
        _ => String::new(),
    }
}

/// Check if a git-based extension has newer commits on remote.
async fn check_upgrade_available(path: &std::path::Path) -> bool {
    // Fetch without updating working tree
    let fetch = tokio::process::Command::new("git")
        .args(["fetch", "--dry-run"])
        .current_dir(path)
        .output()
        .await;

    match fetch {
        Ok(out) => {
            // If stderr contains output, there are updates
            let stderr = String::from_utf8_lossy(&out.stderr);
            !stderr.trim().is_empty()
        }
        Err(_) => false,
    }
}
