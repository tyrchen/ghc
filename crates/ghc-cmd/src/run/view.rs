//! `ghc run view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// View a workflow run.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// The run ID to view.
    #[arg(value_name = "RUN_ID")]
    run_id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Open the run in the browser.
    #[arg(short, long)]
    web: bool,

    /// View the run's log output.
    #[arg(long)]
    log: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the run view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be viewed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/actions/runs/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.run_id,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/actions/runs/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
        );

        let run: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch run")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&run)?);
            return Ok(());
        }

        let name = run.get("name").and_then(Value::as_str).unwrap_or("");
        let status = run.get("status").and_then(Value::as_str).unwrap_or("");
        let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
        let branch = run.get("head_branch").and_then(Value::as_str).unwrap_or("");
        let event = run.get("event").and_then(Value::as_str).unwrap_or("");
        let html_url = run.get("html_url").and_then(Value::as_str).unwrap_or("");
        let run_number = run.get("run_number").and_then(Value::as_u64).unwrap_or(0);
        let created_at = run.get("created_at").and_then(Value::as_str).unwrap_or("");
        let updated_at = run.get("updated_at").and_then(Value::as_str).unwrap_or("");

        let status_display = match (status, conclusion) {
            (_, "success") => cs.success("completed"),
            (_, "failure") => cs.error("failed"),
            (_, "cancelled") => cs.gray("cancelled"),
            ("in_progress", _) => cs.warning("in progress"),
            ("queued", _) => cs.gray("queued"),
            _ => status.to_string(),
        };

        ios_println!(ios, "{}", cs.bold(name));
        ios_println!(ios, "Status: {status_display}");
        ios_println!(ios, "Run #{run_number}");
        ios_println!(ios, "Branch: {branch}");
        ios_println!(ios, "Event: {event}");
        ios_println!(ios, "Started: {created_at}");
        ios_println!(ios, "Updated: {updated_at}");

        // Show jobs
        let jobs_path = format!(
            "repos/{}/{}/actions/runs/{}/jobs",
            repo.owner(),
            repo.name(),
            self.run_id,
        );
        if let Ok(jobs_result) = client
            .rest::<Value>(reqwest::Method::GET, &jobs_path, None)
            .await
            && let Some(jobs) = jobs_result.get("jobs").and_then(Value::as_array)
        {
            ios_println!(ios, "\nJobs:");
            for job in jobs {
                let job_name = job.get("name").and_then(Value::as_str).unwrap_or("");
                let job_conclusion = job.get("conclusion").and_then(Value::as_str).unwrap_or("");
                let job_status = job.get("status").and_then(Value::as_str).unwrap_or("");

                let icon = match job_conclusion {
                    "success" => cs.success_icon(),
                    "failure" => cs.error_icon(),
                    _ if job_status == "in_progress" => cs.warning_icon(),
                    _ => cs.gray("-"),
                };
                ios_println!(ios, "  {icon} {job_name}");
            }
        }

        ios_println!(ios, "\n{}", ghc_core::text::display_url(html_url));

        if self.log {
            let logs_path = format!(
                "repos/{}/{}/actions/runs/{}/logs",
                repo.owner(),
                repo.name(),
                self.run_id,
            );
            let log_content = client
                .rest_text(reqwest::Method::GET, &logs_path, None)
                .await
                .context("failed to fetch run logs")?;
            ios_println!(ios, "\n--- Logs ---\n{log_content}");
        }

        Ok(())
    }
}
