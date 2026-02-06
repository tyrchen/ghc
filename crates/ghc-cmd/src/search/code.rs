//! `ghc search code` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Search for code across GitHub repositories.
#[derive(Debug, Args)]
pub struct CodeArgs {
    /// Search query.
    #[arg(value_name = "QUERY", required = true)]
    query: Vec<String>,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Filter by language.
    #[arg(short, long)]
    language: Option<String>,

    /// Filter by filename.
    #[arg(long)]
    filename: Option<String>,

    /// Filter by file extension.
    #[arg(long)]
    extension: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl CodeArgs {
    /// Run the search code command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");

        if let Some(ref repo) = self.repo {
            let _ = write!(q, " repo:{repo}");
        }
        if let Some(ref lang) = self.language {
            let _ = write!(q, " language:{lang}");
        }
        if let Some(ref fname) = self.filename {
            let _ = write!(q, " filename:{fname}");
        }
        if let Some(ref ext) = self.extension {
            let _ = write!(q, " extension:{ext}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=code");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let encoded = ghc_core::text::percent_encode(&q);
        let path = format!("search/code?q={encoded}&per_page={}", self.limit.min(100),);

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search code")?;

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

        let items = result
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected search response format"))?;

        if items.is_empty() {
            ios_eprintln!(ios, "No code results matched your search");
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in items {
            let repo_name = item
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let file_path = item.get("path").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![cs.bold(repo_name), file_path.to_string()]);
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

    fn default_args(query: &str) -> CodeArgs {
        CodeArgs {
            query: vec![query.to_string()],
            limit: 30,
            repo: None,
            language: None,
            filename: None,
            extension: None,
            json: vec![],
            jq: None,
            template: None,
            web: false,
        }
    }

    fn search_code_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "path": "src/main.rs",
                    "repository": { "full_name": "owner/repo" }
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_code() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/code", search_code_response()).await;

        let args = default_args("fn main");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("owner/repo"), "should contain repo name");
        assert!(out.contains("src/main.rs"), "should contain file path");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("fn main");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=code"),
            "should open search URL with code type"
        );
    }

    #[tokio::test]
    async fn test_should_show_empty_message_when_no_results() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/code",
            serde_json::json!({ "total_count": 0, "items": [] }),
        )
        .await;

        let args = default_args("nonexistent");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("No code results matched"),
            "should show empty message"
        );
    }
}
