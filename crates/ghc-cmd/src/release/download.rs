//! `ghc release download` command.

use std::io::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Download release assets.
///
/// Downloads assets from a GitHub release. By default downloads all assets
/// from the latest release.
///
/// Use `--pattern` to filter assets by glob pattern. Use `--skip-existing`
/// to skip assets that already exist on disk. Use `--clobber` to overwrite
/// existing files.
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

    /// Skip assets that already exist on disk (exit successfully).
    #[arg(long)]
    skip_existing: bool,

    /// Include source code archive (zip/tarball) in the download.
    #[arg(short = 'A', long)]
    archive: Option<String>,
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
            format!("repos/{}/{}/releases/latest", repo.owner(), repo.name())
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

        if let Some(ref archive_format) = self.archive {
            download_archive(
                &client,
                &repo,
                &release,
                archive_format,
                &self.tag,
                &self.dir,
                self.skip_existing,
                self.clobber,
                ios,
            )
            .await?;

            if self.pattern.is_empty() {
                return Ok(());
            }
        }

        download_assets(
            &client,
            assets,
            &self.pattern,
            &self.dir,
            self.skip_existing,
            self.clobber,
            ios,
        )
        .await
    }
}

/// Download individual release assets, applying pattern filters.
#[allow(clippy::too_many_arguments)]
async fn download_assets(
    client: &ghc_api::client::Client,
    assets: &[Value],
    patterns: &[String],
    dir: &str,
    skip_existing: bool,
    clobber: bool,
    ios: &ghc_core::iostreams::IOStreams,
) -> Result<()> {
    let cs = ios.color_scheme();

    if assets.is_empty() {
        ios_eprintln!(ios, "No assets to download");
        return Ok(());
    }

    std::fs::create_dir_all(dir).with_context(|| format!("failed to create directory: {dir}"))?;

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
        if !patterns.is_empty() && !patterns.iter().any(|p| glob_match(p, name)) {
            continue;
        }

        let dest = std::path::Path::new(dir).join(name);
        if dest.exists() && skip_existing {
            ios_eprintln!(
                ios,
                "{} Skipping {name} (already exists)",
                cs.warning_icon(),
            );
            continue;
        }
        if dest.exists() && !clobber {
            ios_eprintln!(
                ios,
                "{} Skipping {name} (already exists, use --clobber to overwrite)",
                cs.warning_icon(),
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

/// Download a source code archive from a release.
#[allow(clippy::too_many_arguments)]
async fn download_archive(
    client: &ghc_api::client::Client,
    repo: &Repo,
    release: &Value,
    archive_format: &str,
    tag_fallback: &str,
    dir: &str,
    skip_existing: bool,
    clobber: bool,
    ios: &ghc_core::iostreams::IOStreams,
) -> Result<()> {
    let cs = ios.color_scheme();
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or(tag_fallback);
    let ext = match archive_format {
        "zip" => "zip",
        "tar.gz" | "tarball" => "tar.gz",
        _ => anyhow::bail!("unsupported archive format: {archive_format} (use zip or tar.gz)"),
    };
    let archive_url = format!(
        "repos/{}/{}/{archive_format}ball/{tag}",
        repo.owner(),
        repo.name(),
    );
    let archive_name = format!("{}-{tag}.{ext}", repo.name());
    let dest = std::path::Path::new(dir).join(&archive_name);

    if dest.exists() && skip_existing {
        ios_eprintln!(
            ios,
            "{} Skipping {archive_name} (already exists)",
            cs.warning_icon(),
        );
        return Ok(());
    }
    if dest.exists() && !clobber {
        ios_eprintln!(
            ios,
            "{} Skipping {archive_name} (already exists, use --clobber to overwrite)",
            cs.warning_icon(),
        );
        return Ok(());
    }

    std::fs::create_dir_all(dir).with_context(|| format!("failed to create directory: {dir}"))?;
    ios_eprintln!(ios, "Downloading {archive_name}...");
    let body = client
        .rest_text(reqwest::Method::GET, &archive_url, None)
        .await
        .with_context(|| format!("failed to download {archive_name}"))?;
    let mut file = std::fs::File::create(&dest)
        .with_context(|| format!("failed to create file: {}", dest.display()))?;
    file.write_all(body.as_bytes())
        .with_context(|| format!("failed to write file: {}", dest.display()))?;
    ios_eprintln!(ios, "{} Downloaded {archive_name}", cs.success_icon());
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- glob_match unit tests ---

    #[test]
    fn test_should_match_exact_pattern() {
        assert!(glob_match("foo.tar.gz", "foo.tar.gz"));
        assert!(!glob_match("foo.tar.gz", "bar.tar.gz"));
    }

    #[test]
    fn test_should_match_wildcard_prefix() {
        assert!(glob_match("*.tar.gz", "foo.tar.gz"));
        assert!(glob_match("*.tar.gz", "bar.tar.gz"));
        assert!(!glob_match("*.tar.gz", "foo.zip"));
    }

    #[test]
    fn test_should_match_wildcard_suffix() {
        assert!(glob_match("foo*", "foo.tar.gz"));
        assert!(glob_match("foo*", "foobar"));
        assert!(!glob_match("foo*", "barfoo"));
    }

    #[test]
    fn test_should_match_wildcard_middle() {
        assert!(glob_match("foo*bar", "foobar"));
        assert!(glob_match("foo*bar", "foo-something-bar"));
        assert!(!glob_match("foo*bar", "foo-baz"));
    }

    #[test]
    fn test_should_match_star_only() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn test_should_match_multiple_wildcards() {
        assert!(glob_match("*linux*amd64*", "myapp-linux-amd64.tar.gz"));
        assert!(!glob_match("*linux*amd64*", "myapp-darwin-arm64.tar.gz"));
    }
}
