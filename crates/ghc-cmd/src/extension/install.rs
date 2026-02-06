//! `ghc extension install` command.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

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

    /// Force install even if already installed.
    #[arg(long)]
    force: bool,
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

        // Handle local install (current directory)
        if self.repo == "." {
            return self.install_local(&extensions_dir).await;
        }

        // Determine repository full name
        let repo_full = if self.repo.contains("://") {
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
        if ext_dir.exists() && !self.force {
            return Err(anyhow::anyhow!(
                "extension {repo_name} is already installed; use --force to reinstall",
            ));
        }

        if ext_dir.exists() && self.force {
            tokio::fs::remove_dir_all(&ext_dir)
                .await
                .context("failed to remove existing extension")?;
        }

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Try to install as a binary extension from releases first
        if try_install_binary(
            factory,
            &repo_full,
            repo_name,
            &ext_dir,
            self.pin.as_deref(),
        )
        .await?
        {
            ios_eprintln!(
                ios,
                "{} Installed binary extension {}",
                cs.success_icon(),
                cs.bold(repo_name),
            );
            return Ok(());
        }

        // Fall back to git clone for script extensions
        self.clone_extension(&repo_full, &ext_dir).await?;

        ios_eprintln!(
            ios,
            "{} Installed extension {}",
            cs.success_icon(),
            cs.bold(repo_name),
        );

        Ok(())
    }

    /// Clone the extension repository.
    async fn clone_extension(&self, repo_full: &str, ext_dir: &Path) -> Result<()> {
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

        Ok(())
    }

    /// Install extension from local directory (symlink).
    async fn install_local(&self, extensions_dir: &Path) -> Result<()> {
        let cwd = std::env::current_dir().context("failed to get current directory")?;
        let dir_name = cwd
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("cannot determine current directory name"))?
            .to_string_lossy()
            .to_string();

        if !dir_name.starts_with("gh-") {
            return Err(anyhow::anyhow!(
                "directory name must start with 'gh-' for local extension install"
            ));
        }

        let link_path = extensions_dir.join(&dir_name);
        if link_path.exists() {
            if self.force {
                let _ = tokio::fs::remove_file(&link_path).await;
                let _ = tokio::fs::remove_dir_all(&link_path).await;
            } else {
                return Err(anyhow::anyhow!(
                    "extension {dir_name} is already installed; use --force to reinstall"
                ));
            }
        }

        #[cfg(unix)]
        {
            tokio::fs::symlink(&cwd, &link_path)
                .await
                .context("failed to create symlink")?;
        }

        #[cfg(windows)]
        {
            tokio::fs::symlink_dir(&cwd, &link_path)
                .await
                .context("failed to create symlink")?;
        }

        Ok(())
    }
}

/// Try to install as a binary extension by downloading a release asset.
///
/// Returns `true` if a binary release was found and installed, `false` otherwise.
async fn try_install_binary(
    factory: &crate::factory::Factory,
    repo_full: &str,
    repo_name: &str,
    ext_dir: &Path,
    pin: Option<&str>,
) -> Result<bool> {
    let client = factory.api_client("github.com")?;

    let release_path = if let Some(tag) = pin {
        format!("repos/{repo_full}/releases/tags/{tag}")
    } else {
        format!("repos/{repo_full}/releases/latest")
    };

    let release: Value = match client.rest(reqwest::Method::GET, &release_path, None).await {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };

    let assets = match release.get("assets").and_then(Value::as_array) {
        Some(a) if !a.is_empty() => a,
        _ => return Ok(false),
    };

    let (os, arch) = current_platform();
    let Some(asset) = find_platform_asset(assets, os, arch) else {
        return Ok(false);
    };

    let download_url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("no download URL for asset"))?;

    let asset_name = asset
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(repo_name);

    download_and_extract(ext_dir, repo_name, download_url, asset_name).await?;

    Ok(true)
}

/// Find a release asset matching the current platform.
fn find_platform_asset<'a>(assets: &'a [Value], os: &str, arch: &str) -> Option<&'a Value> {
    assets.iter().find(|a| {
        let name = a.get("name").and_then(Value::as_str).unwrap_or("");
        let name_lower = name.to_lowercase();
        name_lower.contains(&os.to_lowercase()) && name_lower.contains(&arch.to_lowercase())
    })
}

/// Download and extract (or directly write) a release asset.
async fn download_and_extract(
    ext_dir: &Path,
    repo_name: &str,
    download_url: &str,
    asset_name: &str,
) -> Result<()> {
    let response = reqwest::get(download_url)
        .await
        .context("failed to download release asset")?;

    let bytes = response
        .bytes()
        .await
        .context("failed to read release asset")?;

    tokio::fs::create_dir_all(ext_dir)
        .await
        .context("failed to create extension directory")?;

    let asset_path = Path::new(asset_name);
    let is_tar_gz = asset_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gz") || ext.eq_ignore_ascii_case("tgz"));
    let is_zip = asset_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));

    if is_tar_gz {
        let archive_path = ext_dir.join(asset_name);
        tokio::fs::write(&archive_path, &bytes)
            .await
            .context("failed to write archive")?;
        let status = tokio::process::Command::new("tar")
            .args(["xzf", &archive_path.display().to_string()])
            .current_dir(ext_dir)
            .status()
            .await
            .context("failed to run tar")?;
        if !status.success() {
            return Err(anyhow::anyhow!("failed to extract tar.gz archive"));
        }
        tokio::fs::remove_file(&archive_path).await.ok();
    } else if is_zip {
        let archive_path = ext_dir.join(asset_name);
        tokio::fs::write(&archive_path, &bytes)
            .await
            .context("failed to write archive")?;
        let status = tokio::process::Command::new("unzip")
            .args([
                "-o",
                &archive_path.display().to_string(),
                "-d",
                &ext_dir.display().to_string(),
            ])
            .status()
            .await
            .context("failed to run unzip")?;
        if !status.success() {
            return Err(anyhow::anyhow!("failed to extract zip archive"));
        }
        tokio::fs::remove_file(&archive_path).await.ok();
    } else {
        let bin_path = ext_dir.join(repo_name);
        tokio::fs::write(&bin_path, &bytes)
            .await
            .context("failed to write binary")?;
    }

    // Make files executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut entries = tokio::fs::read_dir(ext_dir)
            .await
            .context("failed to read extension directory")?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                let perms = std::fs::Permissions::from_mode(0o755);
                tokio::fs::set_permissions(&path, perms).await.ok();
            }
        }
    }

    Ok(())
}

/// Get the current OS and architecture for release asset matching.
fn current_platform() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "386"
    } else {
        "amd64"
    };

    (os, arch)
}
