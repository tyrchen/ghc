//! `ghc run watch` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Watch a run until it completes.
#[derive(Debug, Args)]
pub struct WatchArgs {
    /// The run ID to watch.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Polling interval in seconds.
    #[arg(short, long, default_value = "5")]
    interval: u64,

    /// Exit with non-zero status if the run fails.
    #[arg(long)]
    exit_status: bool,
}

impl WatchArgs {
    /// Run the run watch command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be watched.
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
            "repos/{}/{}/actions/runs/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        loop {
            let run: Value = client
                .rest(reqwest::Method::GET, &path, None)
                .await
                .context("failed to fetch run")?;

            let status = run.get("status").and_then(Value::as_str).unwrap_or("");
            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            let name = run.get("name").and_then(Value::as_str).unwrap_or("");

            if status == "completed" {
                let result_display = match conclusion {
                    "success" => cs.success("completed successfully"),
                    "failure" => cs.error("failed"),
                    "cancelled" => cs.gray("was cancelled"),
                    _ => conclusion.to_string(),
                };

                ios_eprintln!(
                    ios,
                    "{} Run {} (#{}) {result_display}",
                    cs.success_icon(),
                    cs.bold(name),
                    self.run_id,
                );

                if self.exit_status && conclusion != "success" {
                    anyhow::bail!("run concluded with: {conclusion}");
                }

                return Ok(());
            }

            let status_display = match status {
                "in_progress" => cs.warning("in progress"),
                "queued" => cs.gray("queued"),
                "waiting" => cs.gray("waiting"),
                _ => status.to_string(),
            };

            ios_eprintln!(
                ios,
                "Run {} (#{}) is {status_display}... (checking again in {}s)",
                cs.bold(name),
                self.run_id,
                self.interval,
            );

            tokio::time::sleep(std::time::Duration::from_secs(self.interval)).await;
        }
    }
}
