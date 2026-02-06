//! `ghc release delete` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a release.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Tag name of the release to delete.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    yes: bool,

    /// Delete the associated tag as well.
    #[arg(long)]
    cleanup_tag: bool,
}

impl DeleteArgs {
    /// Run the release delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release cannot be deleted.
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

        // Delete the release
        let delete_path = format!(
            "repos/{}/{}/releases/{release_id}",
            repo.owner(),
            repo.name(),
        );
        client
            .rest_text(reqwest::Method::DELETE, &delete_path, None)
            .await
            .context("failed to delete release")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted release {} from {}",
            cs.success_icon(),
            cs.bold(&self.tag),
            cs.bold(&repo.full_name()),
        );

        // Optionally delete the tag
        if self.cleanup_tag {
            let tag_path = format!(
                "repos/{}/{}/git/refs/tags/{}",
                repo.owner(),
                repo.name(),
                self.tag,
            );
            client
                .rest_text(reqwest::Method::DELETE, &tag_path, None)
                .await
                .context("failed to delete tag")?;

            ios_eprintln!(
                ios,
                "{} Deleted tag {}",
                cs.success_icon(),
                cs.bold(&self.tag)
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete, mock_rest_get};

    #[tokio::test]
    async fn test_should_delete_release() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "id": 42,
                "tag_name": "v1.0.0",
            }),
        )
        .await;
        mock_rest_delete(&h.server, "/repos/owner/repo/releases/42", 204).await;

        let args = DeleteArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            yes: true,
            cleanup_tag: false,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted release"));
        assert!(err.contains("v1.0.0"));
    }

    #[tokio::test]
    async fn test_should_delete_release_and_tag() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "id": 42,
                "tag_name": "v1.0.0",
            }),
        )
        .await;
        mock_rest_delete(&h.server, "/repos/owner/repo/releases/42", 204).await;
        mock_rest_delete(&h.server, "/repos/owner/repo/git/refs/tags/v1.0.0", 204).await;

        let args = DeleteArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            yes: true,
            cleanup_tag: true,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted release"));
        assert!(err.contains("Deleted tag"));
    }
}
