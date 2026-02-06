//! `ghc cache delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a cache entry.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Cache key or ID to delete.
    #[arg(value_name = "CACHE")]
    cache: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,
}

impl DeleteArgs {
    /// Run the cache delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache entry cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        // Try as numeric ID first, otherwise use key-based deletion
        let path = if self.cache.chars().all(|c| c.is_ascii_digit()) {
            format!(
                "repos/{}/{}/actions/caches/{}",
                repo.owner(),
                repo.name(),
                self.cache,
            )
        } else {
            let encoded = ghc_core::text::percent_encode(&self.cache);
            format!(
                "repos/{}/{}/actions/caches?key={encoded}",
                repo.owner(),
                repo.name(),
            )
        };

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete cache")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted cache {}",
            cs.success_icon(),
            cs.bold(&self.cache),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete};

    #[tokio::test]
    async fn test_should_delete_cache_by_id() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/repos/owner/repo/actions/caches/42", 204).await;

        let args = DeleteArgs {
            cache: "42".to_string(),
            repo: Some("owner/repo".to_string()),
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(stderr.contains("Deleted cache"), "should confirm deletion");
        assert!(stderr.contains("42"), "should contain cache ID");
    }
}
