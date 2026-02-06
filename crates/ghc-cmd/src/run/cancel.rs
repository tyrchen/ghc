//! `ghc run cancel` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Cancel a workflow run.
#[derive(Debug, Args)]
pub struct CancelArgs {
    /// The run ID to cancel.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,
}

impl CancelArgs {
    /// Run the run cancel command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be cancelled.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let path = format!(
            "repos/{}/{}/actions/runs/{}/cancel",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        client
            .rest_text(reqwest::Method::POST, &path, None)
            .await
            .context("failed to cancel run")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Cancelled run {}",
            cs.success_icon(),
            cs.bold(&self.run_id.to_string()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_cancel_run() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/actions/runs/123/cancel",
            202,
            serde_json::json!({}),
        )
        .await;

        let args = CancelArgs {
            run_id: 123,
            repo: Some("owner/repo".to_string()),
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(
            stderr.contains("Cancelled run"),
            "should confirm cancellation"
        );
        assert!(stderr.contains("123"), "should contain run ID");
    }
}
