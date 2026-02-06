//! `ghc workflow view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

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
