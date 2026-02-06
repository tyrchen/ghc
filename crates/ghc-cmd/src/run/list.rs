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
    #[arg(short, long)]
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

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the run list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the runs cannot be listed.
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

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list runs")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let runs = result
            .get("workflow_runs")
            .and_then(Value::as_array)
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
            let status = run.get("status").and_then(Value::as_str).unwrap_or("");
            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            let branch = run.get("head_branch").and_then(Value::as_str).unwrap_or("");
            let event = run.get("event").and_then(Value::as_str).unwrap_or("");
            let created_at = run.get("created_at").and_then(Value::as_str).unwrap_or("");

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
                ("in_progress", _) => cs.warning("in progress"),
                ("queued", _) => cs.gray("queued"),
                _ => status.to_string(),
            };

            tp.add_row(vec![
                status_display,
                cs.bold(name),
                branch.to_string(),
                event.to_string(),
                format!("{id}"),
                created_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
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
            json: vec![],
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
            json: vec![],
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
            json: vec!["id".to_string()],
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("\"workflow_runs\""), "should output JSON");
    }
}
