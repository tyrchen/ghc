//! `ghc workflow view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::text;

/// View details about a workflow.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Workflow ID or filename.
    #[arg(value_name = "WORKFLOW")]
    workflow: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Open the workflow in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,

    /// Display the raw YAML content.
    #[arg(short, long)]
    yaml: bool,
}

impl ViewArgs {
    /// Run the workflow view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the workflow cannot be viewed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/actions/workflows/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.workflow,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/actions/workflows/{}",
            repo.owner(),
            repo.name(),
            self.workflow,
        );

        let wf: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch workflow")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &wf,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let name = wf.get("name").and_then(Value::as_str).unwrap_or("");
        let state = wf.get("state").and_then(Value::as_str).unwrap_or("");
        let id = wf.get("id").and_then(Value::as_u64).unwrap_or(0);
        let wf_path = wf.get("path").and_then(Value::as_str).unwrap_or("");
        let html_url = wf.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_println!(ios, "{}", cs.bold(name));
        ios_println!(ios, "ID: {id}");
        ios_println!(ios, "State: {state}");
        ios_println!(ios, "Path: {wf_path}");

        // Fetch and display recent runs
        let runs_path = format!(
            "repos/{}/{}/actions/workflows/{}/runs?per_page=5",
            repo.owner(),
            repo.name(),
            self.workflow,
        );
        if let Ok(runs_data) = client
            .rest::<Value>(reqwest::Method::GET, &runs_path, None)
            .await
        {
            let total_count = runs_data
                .get("total_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let runs = runs_data
                .get("workflow_runs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            ios_println!(ios, "Total runs: {total_count}");

            if !runs.is_empty() {
                ios_println!(ios, "\n{}", cs.bold("Recent runs"));
                let mut tp = TablePrinter::new(ios);
                for run in &runs {
                    let status = run
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
                    let run_title = run
                        .get("display_title")
                        .or_else(|| run.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let branch = run.get("head_branch").and_then(Value::as_str).unwrap_or("");
                    let event = run.get("event").and_then(Value::as_str).unwrap_or("");
                    let run_id = run.get("id").and_then(Value::as_u64).unwrap_or(0);
                    let created_at = run.get("created_at").and_then(Value::as_str).unwrap_or("");

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
                        text::truncate(run_title, 40),
                        branch.to_string(),
                        event.to_string(),
                        run_id.to_string(),
                        created_at.to_string(),
                    ]);
                }
                ios_println!(ios, "{}", tp.render());
            }
        }

        ios_println!(ios, "\n{}", ghc_core::text::display_url(html_url));

        if self.yaml {
            // Fetch the raw YAML content
            let content_path = format!(
                "repos/{}/{}/contents/{}",
                repo.owner(),
                repo.name(),
                wf_path,
            );
            let content: Value = client
                .rest(reqwest::Method::GET, &content_path, None)
                .await
                .context("failed to fetch workflow file content")?;

            let encoded = content.get("content").and_then(Value::as_str).unwrap_or("");
            if let Ok(bytes) = ghc_core::text::base64_decode(encoded)
                && let Ok(yaml_str) = String::from_utf8(bytes)
            {
                ios_println!(ios, "\n---\n{yaml_str}");
            }
        }

        Ok(())
    }
}
