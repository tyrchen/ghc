//! `ghc release upload` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Upload assets to a release.
#[derive(Debug, Args)]
pub struct UploadArgs {
    /// Tag name of the release.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Files to upload.
    #[arg(value_name = "FILE", required = true)]
    files: Vec<String>,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Overwrite existing assets of the same name.
    #[arg(long)]
    clobber: bool,
}

impl UploadArgs {
    /// Run the release upload command.
    ///
    /// # Errors
    ///
    /// Returns an error if the assets cannot be uploaded.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        // Find the release by tag
        let path = format!(
            "repos/{}/{}/releases/tags/{}",
            repo.owner(),
            repo.name(),
            self.tag,
        );
        let release: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to find release")?;

        let release_id = release
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("release not found for tag {}", self.tag))?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Check for existing assets if clobber is set
        let existing_assets = release
            .get("assets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        for file_path in &self.files {
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or(file_path);

            // Delete existing asset if clobber is enabled
            if self.clobber
                && let Some(existing) = existing_assets.iter().find(|a| {
                    a.get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|n| n == file_name)
                })
                && let Some(asset_id) = existing.get("id").and_then(Value::as_u64)
            {
                let delete_path = format!(
                    "repos/{}/{}/releases/assets/{asset_id}",
                    repo.owner(),
                    repo.name(),
                );
                client
                    .rest_text(reqwest::Method::DELETE, &delete_path, None)
                    .await
                    .with_context(|| format!("failed to delete existing asset: {file_name}"))?;
            }

            let upload_url = format!(
                "https://uploads.github.com/repos/{}/{}/releases/{release_id}/assets?name={file_name}",
                repo.owner(),
                repo.name(),
            );

            ios_eprintln!(ios, "Uploading {file_name}...");

            let _: Value = client
                .rest(reqwest::Method::POST, &upload_url, None)
                .await
                .with_context(|| format!("failed to upload asset: {file_name}"))?;

            ios_eprintln!(ios, "{} Uploaded {file_name}", cs.success_icon());
        }

        Ok(())
    }
}
