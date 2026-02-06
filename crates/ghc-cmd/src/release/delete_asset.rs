//! `ghc release delete-asset` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete an asset from a release.
#[derive(Debug, Args)]
pub struct DeleteAssetArgs {
    /// Tag name of the release.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Name of the asset to delete.
    #[arg(value_name = "ASSET_NAME")]
    asset_name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl DeleteAssetArgs {
    /// Run the release delete-asset command.
    ///
    /// # Errors
    ///
    /// Returns an error if the asset cannot be deleted.
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
        let release: serde_json::Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to find release")?;

        let assets = release
            .get("assets")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("no assets found for release {}", self.tag))?;

        let asset_id = assets
            .iter()
            .find(|a| {
                a.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| n == self.asset_name)
            })
            .and_then(|a| a.get("id"))
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "asset '{}' not found in release {}",
                    self.asset_name,
                    self.tag,
                )
            })?;

        let delete_path = format!(
            "repos/{}/{}/releases/assets/{asset_id}",
            repo.owner(),
            repo.name(),
        );
        client
            .rest_text(reqwest::Method::DELETE, &delete_path, None)
            .await
            .context("failed to delete asset")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted asset {} from release {}",
            cs.success_icon(),
            cs.bold(&self.asset_name),
            cs.bold(&self.tag),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete, mock_rest_get};

    #[tokio::test]
    async fn test_should_delete_asset() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "id": 42,
                "tag_name": "v1.0.0",
                "assets": [
                    {"id": 100, "name": "binary.tar.gz", "size": 1024}
                ]
            }),
        )
        .await;
        mock_rest_delete(&h.server, "/repos/owner/repo/releases/assets/100", 204).await;

        let args = DeleteAssetArgs {
            tag: "v1.0.0".into(),
            asset_name: "binary.tar.gz".into(),
            repo: Some("owner/repo".into()),
            yes: true,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted asset"));
        assert!(err.contains("binary.tar.gz"));
    }

    #[tokio::test]
    async fn test_should_fail_when_asset_not_found() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "id": 42,
                "tag_name": "v1.0.0",
                "assets": []
            }),
        )
        .await;

        let args = DeleteAssetArgs {
            tag: "v1.0.0".into(),
            asset_name: "nonexistent.tar.gz".into(),
            repo: Some("owner/repo".into()),
            yes: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("nonexistent.tar.gz")
        );
    }
}
