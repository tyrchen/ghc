//! `ghc run view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// View a workflow run.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Show job steps.
    #[arg(short, long)]
    verbose: bool,

    /// View the run's log output.
    #[arg(long)]
    log: bool,

    /// View the log for any failed steps in a run or specific job.
    #[arg(long)]
    log_failed: bool,

    /// View a specific job ID from a run.
    #[arg(short, long)]
    job: Option<String>,

    /// Exit with non-zero status if run failed.
    #[arg(long)]
    exit_status: bool,

    /// The attempt number of the workflow run.
    #[arg(short, long)]
    attempt: Option<u64>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ViewArgs {
    /// Run the run view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the run cannot be viewed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        if self.web && self.log {
            anyhow::bail!("specify only one of --web or --log");
        }
        if self.log && self.log_failed {
            anyhow::bail!("specify only one of --log or --log-failed");
        }

        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;

        if self.web {
            let url = if let Some(ref job_id) = self.job {
                format!(
                    "https://{}/{}/{}/actions/runs/{}?check_suite_focus=true#step:1:1",
                    repo.host(),
                    repo.owner(),
                    repo.name(),
                    job_id,
                )
            } else {
                format!(
                    "https://{}/{}/{}/actions/runs/{}",
                    repo.host(),
                    repo.owner(),
                    repo.name(),
                    self.run_id,
                )
            };
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let mut path = format!(
            "repos/{}/{}/actions/runs/{}",
            repo.owner(),
            repo.name(),
            self.run_id,
        );
        if let Some(attempt) = self.attempt {
            path = format!("{path}/attempts/{attempt}");
        }

        let run: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch run")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &run,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let name = run.get("name").and_then(Value::as_str).unwrap_or("");
        let display_title = run
            .get("display_title")
            .and_then(Value::as_str)
            .unwrap_or(name);
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

        ios_println!(ios, "{}", cs.bold(display_title));
        ios_println!(ios, "{name} - {status_display}");
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
                let job_id = job.get("id").and_then(Value::as_u64).unwrap_or(0);
                let job_conclusion = job.get("conclusion").and_then(Value::as_str).unwrap_or("");
                let job_status = job.get("status").and_then(Value::as_str).unwrap_or("");
                let job_started = job.get("started_at").and_then(Value::as_str).unwrap_or("");
                let job_completed = job
                    .get("completed_at")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                let icon = match job_conclusion {
                    "success" => cs.success_icon(),
                    "failure" => cs.error_icon(),
                    _ if job_status == "in_progress" => cs.warning_icon(),
                    _ => cs.gray("-"),
                };

                let duration = format_elapsed(job_started, job_completed);
                let duration_display = if duration.is_empty() {
                    String::new()
                } else {
                    format!(" ({duration})")
                };

                ios_println!(ios, "  {icon} {job_name} (ID {job_id}){duration_display}",);

                // Show steps in verbose mode
                if self.verbose
                    && let Some(steps) = job.get("steps").and_then(Value::as_array)
                {
                    for step in steps {
                        let step_name = step
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        let step_conclusion =
                            step.get("conclusion").and_then(Value::as_str).unwrap_or("");
                        let step_status = step.get("status").and_then(Value::as_str).unwrap_or("");

                        let step_icon = match (step_status, step_conclusion) {
                            ("completed", "success") => cs.success_icon(),
                            ("completed", "failure") => cs.error_icon(),
                            ("completed", "skipped") => cs.gray("o"),
                            ("in_progress", _) => cs.warning_icon(),
                            _ => cs.gray("."),
                        };
                        ios_println!(ios, "    {step_icon} {step_name}");
                    }
                }
            }
        }

        ios_println!(ios, "\n{}", ghc_core::text::display_url(html_url));

        if self.log || self.log_failed {
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

            if self.log_failed {
                // Only show log lines around failures
                ios_println!(ios, "\n--- Failed Step Logs ---");
                for line in log_content.lines() {
                    if line.contains("##[error]") || line.contains("Process completed with exit") {
                        ios_println!(ios, "{line}");
                    }
                }
            } else {
                ios_println!(ios, "\n--- Logs ---\n{log_content}");
            }
        }

        // Exit with non-zero status if run failed
        if self.exit_status && conclusion == "failure" {
            anyhow::bail!("run concluded with: {conclusion}");
        }

        Ok(())
    }
}

/// Format elapsed time between two ISO 8601 timestamps.
fn format_elapsed(start: &str, end: &str) -> String {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start).ok();
    let end_dt = chrono::DateTime::parse_from_rfc3339(end).ok();

    match (start_dt, end_dt) {
        (Some(s), Some(e)) => {
            let secs = (e - s).num_seconds().max(0);
            if secs < 60 {
                format!("{secs}s")
            } else if secs < 3600 {
                format!("{}m{}s", secs / 60, secs % 60)
            } else {
                format!("{}h{}m{}s", secs / 3600, (secs % 3600) / 60, secs % 60)
            }
        }
        _ => String::new(),
    }
}
