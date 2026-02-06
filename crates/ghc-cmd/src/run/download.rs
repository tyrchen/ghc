//! `ghc run download` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Download artifacts from a workflow run.
#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// The run ID to download artifacts from.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Download only artifacts matching a name pattern.
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,

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

        for artifact in artifacts {
            let name = artifact
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let artifact_id = artifact.get("id").and_then(Value::as_u64).unwrap_or(0);

            if let Some(ref filter) = self.name
                && name != filter
            {
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
        }

        Ok(())
    }
}
