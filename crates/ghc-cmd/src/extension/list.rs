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

            // Try to determine version from git
            let version = get_extension_version(&entry.path()).await;

            tp.add_row(vec![cs.bold(&name), version]);
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
    let output = tokio::process::Command::new("git")
        .args(["describe", "--tags", "--always"])
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::from("unknown"),
    }
}
