//! `ghc run watch` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::iostreams::{ColorScheme, IOStreams};
use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Watch a run until it completes.
///
/// Displays real-time status updates with job-level detail, showing
/// which jobs are queued, in progress, or completed.
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

        let run_path = format!(
            "repos/{}/{}/actions/runs/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        let jobs_path = format!(
            "repos/{}/{}/actions/runs/{}/jobs",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        loop {
            let run: Value = client
                .rest(reqwest::Method::GET, &run_path, None)
                .await
                .context("failed to fetch run")?;

            let status = run.get("status").and_then(Value::as_str).unwrap_or("");
            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            let name = run.get("name").and_then(Value::as_str).unwrap_or("");

            // Fetch and display jobs
            if let Ok(jobs_data) = client
                .rest::<Value>(reqwest::Method::GET, &jobs_path, None)
                .await
            {
                display_jobs(ios, &cs, &jobs_data, name, self.run_id);
            }

            if status == "completed" {
                let result_display = match conclusion {
                    "success" => cs.success("completed successfully"),
                    "failure" => cs.error("failed"),
                    "cancelled" => cs.gray("was cancelled"),
                    _ => conclusion.to_string(),
                };

                ios_println!(ios, "");
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

/// Display jobs and their status for a workflow run.
fn display_jobs(ios: &IOStreams, cs: &ColorScheme, jobs_data: &Value, name: &str, run_id: u64) {
    let jobs = jobs_data
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if jobs.is_empty() {
        return;
    }

    ios_eprintln!(ios, "");
    ios_eprintln!(ios, "Jobs for {} (#{})", cs.bold(name), run_id);

    for job in &jobs {
        let job_name = job.get("name").and_then(Value::as_str).unwrap_or("unknown");
        let job_status = job.get("status").and_then(Value::as_str).unwrap_or("");
        let job_conclusion = job.get("conclusion").and_then(Value::as_str).unwrap_or("");

        let status_icon = match (job_status, job_conclusion) {
            ("completed", "success") => cs.success("v"),
            ("completed", "failure") => cs.error("X"),
            ("completed", "cancelled") => cs.gray("-"),
            ("completed", "skipped") => cs.gray("o"),
            ("in_progress", _) => cs.warning("*"),
            ("queued" | "waiting", _) => cs.gray("."),
            _ => " ".to_string(),
        };

        ios_eprintln!(ios, "  {status_icon} {job_name}");

        // Show steps for in-progress jobs
        if job_status == "in_progress"
            && let Some(steps) = job.get("steps").and_then(Value::as_array)
        {
            print_steps(ios, cs, steps);
        }
    }
}

/// Print step-level details for an in-progress job.
fn print_steps(ios: &IOStreams, cs: &ColorScheme, steps: &[Value]) {
    for step in steps {
        let step_name = step
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let step_status = step.get("status").and_then(Value::as_str).unwrap_or("");
        let step_conclusion = step.get("conclusion").and_then(Value::as_str).unwrap_or("");

        let step_icon = match (step_status, step_conclusion) {
            ("completed", "success") => cs.success("v"),
            ("completed", "failure") => cs.error("X"),
            ("completed", "skipped") => cs.gray("o"),
            ("in_progress", _) => cs.warning("*"),
            _ => cs.gray("."),
        };

        ios_eprintln!(ios, "    {step_icon} {step_name}");
    }
}
