//! `ghc codespace list` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List codespaces.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Maximum number of codespaces to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

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
    /// Run the codespace list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespaces cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let mut path = format!("user/codespaces?per_page={}", self.limit.min(100));
        if let Some(ref repo) = self.repo {
            let encoded = ghc_core::text::percent_encode(repo);
            let _ = write!(path, "&repository_id={encoded}");
        }

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list codespaces")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &result,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let codespaces = result
            .get("codespaces")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if codespaces.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No codespaces found");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for codespace in codespaces {
            let name = codespace.get("name").and_then(Value::as_str).unwrap_or("");
            let display_name = codespace
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(name);
            let repo_name = codespace
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let state = codespace.get("state").and_then(Value::as_str).unwrap_or("");
            let branch = codespace
                .pointer("/git_status/ref")
                .and_then(Value::as_str)
                .unwrap_or("");

            let state_display = match state {
                "Available" => cs.success("available"),
                "Shutdown" => cs.gray("stopped"),
                "Rebuilding" | "Starting" => cs.warning(state),
                _ => state.to_string(),
            };

            tp.add_row(vec![
                cs.bold(display_name),
                repo_name.to_string(),
                branch.to_string(),
                state_display,
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
    async fn test_should_list_codespaces() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user/codespaces",
            serde_json::json!({
                "total_count": 2,
                "codespaces": [
                    {
                        "name": "my-codespace-abc",
                        "display_name": "My Codespace",
                        "state": "Available",
                        "repository": {"full_name": "owner/repo"},
                        "git_status": {"ref": "main"}
                    },
                    {
                        "name": "other-codespace-xyz",
                        "display_name": "Other Space",
                        "state": "Shutdown",
                        "repository": {"full_name": "owner/other"},
                        "git_status": {"ref": "feature"}
                    }
                ]
            }),
        )
        .await;

        let args = ListArgs {
            limit: 30,
            repo: None,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(
            stdout.contains("My Codespace"),
            "should contain codespace display name"
        );
        assert!(stdout.contains("owner/repo"), "should contain repo name");
        assert!(
            stdout.contains("Other Space"),
            "should contain second codespace"
        );
    }
}
