//! `ghc release download` command.

use std::io::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Download release assets.
#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Tag name of the release (or "latest").
    #[arg(value_name = "TAG", default_value = "latest")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Download only assets matching a glob pattern.
    #[arg(short, long, value_name = "PATTERN")]
    pattern: Vec<String>,

    /// Directory to download into.
    #[arg(short = 'D', long, default_value = ".")]
    dir: String,

    /// Overwrite existing files.
    #[arg(long)]
    clobber: bool,
}

impl DownloadArgs {
    /// Run the release download command.
    ///
    /// # Errors
    ///
    /// Returns an error if the assets cannot be downloaded.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let path = if self.tag == "latest" {
            format!("repos/{}/{}/releases/latest", repo.owner(), repo.name(),)
        } else {
            format!(
                "repos/{}/{}/releases/tags/{}",
                repo.owner(),
                repo.name(),
                self.tag,
            )
        };

        let release: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to find release")?;

        let assets = release
            .get("assets")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("no assets found for release"))?;

        let ios = &factory.io;

        if assets.is_empty() {
            ios_eprintln!(ios, "No assets to download");
            return Ok(());
        }

        let cs = ios.color_scheme();
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create directory: {}", self.dir))?;

        for asset in assets {
            let name = asset
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let download_url = asset
                .get("browser_download_url")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("no download URL for asset {name}"))?;

            // Apply pattern filter
            if !self.pattern.is_empty() && !self.pattern.iter().any(|p| glob_match(p, name)) {
                continue;
            }

            let dest = std::path::Path::new(&self.dir).join(name);
            if dest.exists() && !self.clobber {
                ios_eprintln!(
                    ios,
                    "{} Skipping {name} (already exists)",
                    cs.warning_icon()
                );
                continue;
            }

            ios_eprintln!(ios, "Downloading {name}...");

            let body = client
                .rest_text(reqwest::Method::GET, download_url, None)
                .await
                .with_context(|| format!("failed to download {name}"))?;

            let mut file = std::fs::File::create(&dest)
                .with_context(|| format!("failed to create file: {}", dest.display()))?;
            file.write_all(body.as_bytes())
                .with_context(|| format!("failed to write file: {}", dest.display()))?;

            ios_eprintln!(ios, "{} Downloaded {name}", cs.success_icon());
        }

        Ok(())
    }
}

/// Simple glob matching supporting `*` wildcards.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        return pattern == text;
    }

    let mut remaining = text;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
            return true;
        } else {
            match remaining.find(part) {
                Some(pos) => remaining = &remaining[pos + part.len()..],
                None => return false,
            }
        }
    }

    true
}
