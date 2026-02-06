//! `ghc run rerun` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Rerun a workflow run.
#[derive(Debug, Args)]
pub struct RerunArgs {
    /// The run ID to rerun.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Only rerun failed jobs.
    #[arg(long)]
    failed: bool,

    /// Enable debug logging for the rerun.
    #[arg(short, long)]
    debug: bool,
}

impl RerunArgs {
    /// Run the run rerun command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be rerun.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let endpoint = if self.failed {
            "rerun-failed-jobs"
        } else {
            "rerun"
        };

        let path = format!(
            "repos/{}/{}/actions/runs/{}/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
            endpoint,
        );

        let body = if self.debug {
            Some(serde_json::json!({ "enable_debug_logging": true }))
        } else {
            None
        };

        client
            .rest_text(reqwest::Method::POST, &path, body.as_ref())
            .await
            .context("failed to rerun workflow")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        let action = if self.failed {
            "Rerun failed jobs for"
        } else {
            "Rerun"
        };
        ios_eprintln!(
            ios,
            "{} {action} run {}",
            cs.success_icon(),
            cs.bold(&self.run_id.to_string()),
        );

        Ok(())
    }
}
