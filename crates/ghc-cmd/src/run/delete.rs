//! `ghc run delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a workflow run.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The run ID to delete.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,
}

impl DeleteArgs {
    /// Run the run delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let path = format!(
            "repos/{}/{}/actions/runs/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete run")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted run {}",
            cs.success_icon(),
            cs.bold(&self.run_id.to_string()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete};

    #[tokio::test]
    async fn test_should_delete_run() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/repos/owner/repo/actions/runs/456", 204).await;

        let args = DeleteArgs {
            run_id: 456,
            repo: Some("owner/repo".to_string()),
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(stderr.contains("Deleted run"), "should confirm deletion");
        assert!(stderr.contains("456"), "should contain run ID");
    }
}
