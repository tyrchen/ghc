//! `ghc workflow list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List workflows in a repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of workflows to list.
    #[arg(short = 'L', long, default_value = "50")]
    limit: u32,

    /// Show all workflows including disabled ones.
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
    /// Run the workflow list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the workflows cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let path = format!(
            "repos/{}/{}/actions/workflows?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit.min(100),
        );

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list workflows")?;

        // Extract inner array from wrapper object
        let items = result
            .get("workflows")
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

        let workflows = items
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if workflows.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No workflows found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for wf in workflows {
            let name = wf.get("name").and_then(Value::as_str).unwrap_or("");
            let state = wf.get("state").and_then(Value::as_str).unwrap_or("");
            let id = wf.get("id").and_then(Value::as_u64).unwrap_or(0);
            let file_name = wf.get("path").and_then(Value::as_str).unwrap_or("");

            if !self.all && state == "disabled_manually" {
                continue;
            }

            let state_display = match state {
                "active" => cs.success("active"),
                "disabled_manually" => cs.warning("disabled"),
                "disabled_inactivity" => cs.gray("disabled (inactivity)"),
                _ => state.to_string(),
            };

            tp.add_row(vec![
                cs.bold(name),
                state_display,
                format!("{id}"),
                file_name.to_string(),
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
    async fn test_should_list_workflows() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/workflows",
            serde_json::json!({
                "total_count": 2,
                "workflows": [
                    {"id": 1, "name": "CI", "state": "active", "path": ".github/workflows/ci.yml"},
                    {"id": 2, "name": "Deploy", "state": "active", "path": ".github/workflows/deploy.yml"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            limit: 50,
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
        assert!(stdout.contains("ci.yml"), "should contain workflow path");
    }

    #[tokio::test]
    async fn test_should_filter_disabled_workflows() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/workflows",
            serde_json::json!({
                "total_count": 2,
                "workflows": [
                    {"id": 1, "name": "CI", "state": "active", "path": ".github/workflows/ci.yml"},
                    {"id": 2, "name": "Old", "state": "disabled_manually", "path": ".github/workflows/old.yml"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            limit: 50,
            all: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("CI"), "should contain active workflow");
        assert!(
            !stdout.contains("Old"),
            "should not contain disabled workflow"
        );
    }
}
