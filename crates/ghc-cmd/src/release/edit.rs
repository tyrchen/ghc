//! `ghc release edit` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Edit a release.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Tag name of the release to edit.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// New release title.
    #[arg(short, long)]
    title: Option<String>,

    /// New release notes.
    #[arg(short, long)]
    notes: Option<String>,

    /// Read release notes from a file.
    #[arg(short = 'F', long, value_name = "FILE")]
    notes_file: Option<String>,

    /// Set or unset draft status.
    #[arg(long)]
    draft: Option<bool>,

    /// Set or unset prerelease status.
    #[arg(long)]
    prerelease: Option<bool>,

    /// Set as latest release.
    #[arg(long)]
    latest: Option<bool>,

    /// New tag name.
    #[arg(long)]
    tag_name: Option<String>,
}

impl EditArgs {
    /// Run the release edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release cannot be edited.
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

        let mut body = serde_json::json!({});

        if let Some(ref title) = self.title {
            body["name"] = Value::String(title.clone());
        }
        if let Some(ref tag_name) = self.tag_name {
            body["tag_name"] = Value::String(tag_name.clone());
        }
        if let Some(ref notes) = self.notes {
            body["body"] = Value::String(notes.clone());
        }
        if let Some(ref notes_file) = self.notes_file {
            let content = std::fs::read_to_string(notes_file)
                .with_context(|| format!("failed to read {notes_file}"))?;
            body["body"] = Value::String(content);
        }
        if let Some(draft) = self.draft {
            body["draft"] = Value::Bool(draft);
        }
        if let Some(prerelease) = self.prerelease {
            body["prerelease"] = Value::Bool(prerelease);
        }
        if let Some(latest) = self.latest {
            body["make_latest"] = Value::String(latest.to_string());
        }

        let edit_path = format!(
            "repos/{}/{}/releases/{release_id}",
            repo.owner(),
            repo.name(),
        );
        let result: Value = client
            .rest(reqwest::Method::PATCH, &edit_path, Some(&body))
            .await
            .context("failed to edit release")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Edited release {} in {}",
            cs.success_icon(),
            cs.bold(&self.tag),
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{html_url}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_patch};

    #[tokio::test]
    async fn test_should_edit_release() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "id": 42,
                "tag_name": "v1.0.0",
                "name": "Release 1.0",
            }),
        )
        .await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/releases/42",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
            }),
        )
        .await;

        let args = EditArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            title: Some("Updated Title".into()),
            notes: None,
            notes_file: None,
            draft: None,
            prerelease: None,
            latest: None,
            tag_name: None,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Edited release"));
        assert!(err.contains("v1.0.0"));
    }
}
