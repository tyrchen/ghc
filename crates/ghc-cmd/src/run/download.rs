//! `ghc run download` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Download artifacts from a workflow run.
///
/// Downloads workflow run artifacts. Use `--name` for exact name match
/// or `--pattern` for glob pattern matching (e.g. `--pattern "build-*"`).
#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// The run ID to download artifacts from.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Download only artifacts matching an exact name.
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,

    /// Download only artifacts matching a glob pattern.
    #[arg(short, long, value_name = "PATTERN")]
    pattern: Vec<String>,

    /// Directory to download into.
    #[arg(short = 'D', long, default_value = ".")]
    dir: String,
}

impl DownloadArgs {
    /// Run the run download command.
    ///
    /// # Errors
    ///
    /// Returns an error if the artifacts cannot be downloaded.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/actions/runs/{}/artifacts",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list artifacts")?;

        let artifacts = result
            .get("artifacts")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("no artifacts found for run {}", self.run_id))?;

        if artifacts.is_empty() {
            ios_eprintln!(ios, "No artifacts found for run {}", self.run_id);
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create directory: {}", self.dir))?;

        let mut downloaded = 0u32;

        for artifact in artifacts {
            let name = artifact
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let artifact_id = artifact.get("id").and_then(Value::as_u64).unwrap_or(0);

            // Exact name filter
            if let Some(ref filter) = self.name
                && name != filter
            {
                continue;
            }

            // Glob pattern filter
            if !self.pattern.is_empty() && !self.pattern.iter().any(|p| glob_match(p, name)) {
                continue;
            }

            let download_path = format!(
                "repos/{}/{}/actions/artifacts/{artifact_id}/zip",
                repo.owner(),
                repo.name(),
            );

            ios_eprintln!(ios, "Downloading {name}...");

            let content = client
                .rest_text(reqwest::Method::GET, &download_path, None)
                .await
                .with_context(|| format!("failed to download artifact: {name}"))?;

            let dest = std::path::Path::new(&self.dir).join(format!("{name}.zip"));
            std::fs::write(&dest, content.as_bytes())
                .with_context(|| format!("failed to write file: {}", dest.display()))?;

            ios_eprintln!(ios, "{} Downloaded {name}", cs.success_icon());
            downloaded += 1;
        }

        if downloaded == 0 {
            ios_eprintln!(ios, "No artifacts matched the specified filters");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_match_glob_patterns() {
        assert!(glob_match("build-*", "build-linux"));
        assert!(glob_match("build-*", "build-macos-arm64"));
        assert!(!glob_match("build-*", "test-linux"));
        assert!(glob_match("*-linux-*", "build-linux-amd64"));
        assert!(!glob_match("*-linux-*", "build-macos-arm64"));
    }

    #[test]
    fn test_should_match_exact_name() {
        assert!(glob_match("my-artifact", "my-artifact"));
        assert!(!glob_match("my-artifact", "other-artifact"));
    }
}
