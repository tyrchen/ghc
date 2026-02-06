//! `ghc release create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Create a new release.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateArgs {
    /// Tag name for the release.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Release title.
    #[arg(short, long)]
    title: Option<String>,

    /// Release notes.
    #[arg(short, long)]
    notes: Option<String>,

    /// Read release notes from a file.
    #[arg(short = 'F', long, value_name = "FILE")]
    notes_file: Option<String>,

    /// Target branch or commit SHA.
    #[arg(long)]
    target: Option<String>,

    /// Mark as a draft release.
    #[arg(short, long)]
    draft: bool,

    /// Mark as a prerelease.
    #[arg(short, long)]
    prerelease: bool,

    /// Generate release notes automatically.
    #[arg(long)]
    generate_notes: bool,

    /// Mark as the latest release.
    #[arg(long)]
    latest: bool,

    /// Files to upload as release assets.
    #[arg(value_name = "FILE")]
    files: Vec<String>,
}

impl CreateArgs {
    /// Run the release create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = resolve_repo(self.repo.as_deref())?;
        let client = factory.api_client(repo.host())?;

        let notes_body = match (&self.notes, &self.notes_file) {
            (Some(n), _) => n.clone(),
            (_, Some(f)) => {
                std::fs::read_to_string(f).with_context(|| format!("failed to read {f}"))?
            }
            _ => String::new(),
        };

        let mut body = serde_json::json!({
            "tag_name": self.tag,
            "name": self.title.as_deref().unwrap_or(&self.tag),
            "body": notes_body,
            "draft": self.draft,
            "prerelease": self.prerelease,
            "generate_release_notes": self.generate_notes,
        });

        if let Some(ref target) = self.target {
            body["target_commitish"] = Value::String(target.clone());
        }

        if self.latest {
            body["make_latest"] = Value::String("true".to_string());
        }

        let path = format!("repos/{}/{}/releases", repo.owner(), repo.name(),);
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to create release")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");
        let release_id = result.get("id").and_then(Value::as_u64).unwrap_or(0);

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Upload assets if provided
        for file_path in &self.files {
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or(file_path);

            let _data = std::fs::read(file_path)
                .with_context(|| format!("failed to read asset: {file_path}"))?;

            let upload_url = format!(
                "https://uploads.github.com/repos/{}/{}/releases/{release_id}/assets?name={file_name}",
                repo.owner(),
                repo.name(),
            );

            let _: Value = client
                .rest(reqwest::Method::POST, &upload_url, None)
                .await
                .with_context(|| format!("failed to upload asset: {file_name}"))?;

            ios_eprintln!(ios, "{} Uploaded {file_name}", cs.success_icon());
        }

        if self.draft {
            ios_eprintln!(
                ios,
                "{} Created draft release {} for {}",
                cs.success_icon(),
                cs.bold(&self.tag),
                cs.bold(&repo.full_name()),
            );
        } else {
            ios_eprintln!(
                ios,
                "{} Created release {} for {}",
                cs.success_icon(),
                cs.bold(&self.tag),
                cs.bold(&repo.full_name()),
            );
        }
        ios_println!(ios, "{html_url}");

        Ok(())
    }
}

/// Resolve a repository from the `--repo` flag or bail.
fn resolve_repo(repo_flag: Option<&str>) -> Result<Repo> {
    let name = repo_flag
        .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
    Repo::from_full_name(name).context("invalid repository format")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_release() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 1,
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "tag_name": "v1.0.0",
            }),
        )
        .await;

        let args = CreateArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            title: Some("Release 1.0".into()),
            notes: Some("Initial release".into()),
            notes_file: None,
            target: None,
            draft: false,
            prerelease: false,
            generate_notes: false,
            latest: false,
            files: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created release"));
        assert!(err.contains("v1.0.0"));
        let out = h.stdout();
        assert!(out.contains("https://github.com/owner/repo/releases/tag/v1.0.0"));
    }

    #[tokio::test]
    async fn test_should_create_draft_release() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 2,
                "html_url": "https://github.com/owner/repo/releases/tag/v2.0.0",
                "tag_name": "v2.0.0",
            }),
        )
        .await;

        let args = CreateArgs {
            tag: "v2.0.0".into(),
            repo: Some("owner/repo".into()),
            title: None,
            notes: None,
            notes_file: None,
            target: None,
            draft: true,
            prerelease: false,
            generate_notes: false,
            latest: false,
            files: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created draft release"));
    }
}
