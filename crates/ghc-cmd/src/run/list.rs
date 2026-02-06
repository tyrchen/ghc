//! `ghc run list` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List recent workflow runs.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of runs to list.
    #[arg(short = 'L', long, default_value = "20")]
    limit: u32,

    /// Filter by workflow name or ID.
    #[arg(short, long)]
    workflow: Option<String>,

    /// Filter by branch.
    #[arg(short, long)]
    branch: Option<String>,

    /// Filter by actor (user who triggered the run).
    #[arg(short = 'u', long = "user")]
    actor: Option<String>,

    /// Filter by status.
    #[arg(
        short,
        long,
        value_parser = ["completed", "in_progress", "queued", "waiting", "requested"]
    )]
    status: Option<String>,

    /// Filter by event.
    #[arg(short, long)]
    event: Option<String>,

    /// Filter runs by the date it was created.
    #[arg(long)]
    created: Option<String>,

    /// Filter runs by the SHA of the commit.
    #[arg(short, long)]
    commit: Option<String>,

    /// Include disabled workflows.
    #[arg(short, long)]
    all: bool,

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

impl ListArgs {
    /// Run the run list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the runs cannot be listed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let mut path = format!(
            "repos/{}/{}/actions/runs?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit.min(100),
        );
        if let Some(ref branch) = self.branch {
            let _ = write!(path, "&branch={branch}");
        }
        if let Some(ref actor) = self.actor {
            let _ = write!(path, "&actor={actor}");
        }
        if let Some(ref status) = self.status {
            let _ = write!(path, "&status={status}");
        }
        if let Some(ref event) = self.event {
            let _ = write!(path, "&event={event}");
        }
        if let Some(ref created) = self.created {
            let _ = write!(path, "&created={created}");
        }
        if let Some(ref commit) = self.commit {
            let _ = write!(path, "&head_sha={commit}");
        }
        if self.all {
            let _ = write!(path, "&exclude_pull_requests=false");
        }

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list runs")?;

        // Extract inner array from wrapper object
        let items = result
            .get("workflow_runs")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &items,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let runs = items
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if runs.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No runs found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for run in runs {
            let id = run.get("id").and_then(Value::as_u64).unwrap_or(0);
            let name = run.get("name").and_then(Value::as_str).unwrap_or("");
            let display_title = run
                .get("display_title")
                .and_then(Value::as_str)
                .unwrap_or(name);
            let status = run.get("status").and_then(Value::as_str).unwrap_or("");
            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            let branch = run.get("head_branch").and_then(Value::as_str).unwrap_or("");
            let event = run.get("event").and_then(Value::as_str).unwrap_or("");
            let created_at = run.get("created_at").and_then(Value::as_str).unwrap_or("");
            let updated_at = run.get("updated_at").and_then(Value::as_str).unwrap_or("");

            // Filter by workflow name if specified
            if let Some(ref wf_filter) = self.workflow
                && !name.eq_ignore_ascii_case(wf_filter)
            {
                continue;
            }

            let status_display = match (status, conclusion) {
                (_, "success") => cs.success("completed"),
                (_, "failure") => cs.error("failed"),
                (_, "cancelled") => cs.gray("cancelled"),
                (_, "skipped") => cs.gray("skipped"),
                ("in_progress", _) => cs.warning("in progress"),
                ("queued", _) => cs.gray("queued"),
                ("waiting", _) => cs.gray("waiting"),
                _ => status.to_string(),
            };

            let elapsed = format_elapsed(created_at, updated_at);
            let age = format_age(created_at);

            tp.add_row(vec![
                status_display,
                cs.bold(display_title),
                name.to_string(),
                branch.to_string(),
                event.to_string(),
                cs.cyan(&format!("{id}")),
                elapsed,
                age,
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

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

/// Format how long ago a timestamp was.
fn format_age(timestamp: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(timestamp) {
        Ok(dt) => {
            let duration = chrono::Utc::now().signed_duration_since(dt);
            ghc_core::text::fuzzy_ago(duration)
        }
        Err(_) => timestamp.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_workflow_runs() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/runs",
            serde_json::json!({
                "total_count": 2,
                "workflow_runs": [
                    {
                        "id": 100,
                        "name": "CI",
                        "status": "completed",
                        "conclusion": "success",
                        "head_branch": "main",
                        "event": "push",
                        "created_at": "2024-01-15T10:00:00Z"
                    },
                    {
                        "id": 101,
                        "name": "Deploy",
                        "status": "in_progress",
                        "conclusion": null,
                        "head_branch": "release",
                        "event": "workflow_dispatch",
                        "created_at": "2024-01-15T11:00:00Z"
                    }
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            limit: 20,
            workflow: None,
            branch: None,
            actor: None,
            status: None,
            event: None,
            created: None,
            commit: None,
            all: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("CI"), "should contain workflow name CI");
        assert!(
            stdout.contains("Deploy"),
            "should contain workflow name Deploy"
        );
        assert!(stdout.contains("main"), "should contain branch name");
        assert!(stdout.contains("push"), "should contain event type");
    }

    #[tokio::test]
    async fn test_should_require_repo_argument() {
        let h = TestHarness::new().await;
        let args = ListArgs {
            repo: None,
            limit: 20,
            workflow: None,
            branch: None,
            actor: None,
            status: None,
            event: None,
            created: None,
            commit: None,
            all: false,
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("repository argument required")
        );
    }

    #[tokio::test]
    async fn test_should_output_json_when_flag_set() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/runs",
            serde_json::json!({
                "total_count": 1,
                "workflow_runs": [
                    {"id": 100, "name": "CI", "status": "completed", "conclusion": "success"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            limit: 20,
            workflow: None,
            branch: None,
            actor: None,
            status: None,
            event: None,
            created: None,
            commit: None,
            all: false,
            json: vec!["id".to_string(), "name".to_string()],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(
            stdout.contains("\"id\""),
            "should output JSON with id field: {stdout}",
        );
        assert!(
            stdout.contains("\"name\""),
            "should output JSON with name field: {stdout}",
        );
    }
}
