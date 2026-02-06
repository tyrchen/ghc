//! `ghc search issues` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Search for issues across GitHub.
#[derive(Debug, Args)]
pub struct IssuesArgs {
    /// Search query.
    #[arg(value_name = "QUERY", required = true)]
    query: Vec<String>,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Filter by state.
    #[arg(long, value_parser = ["open", "closed"])]
    state: Option<String>,

    /// Filter by assignee.
    #[arg(long)]
    assignee: Option<String>,

    /// Filter by label.
    #[arg(long)]
    label: Vec<String>,

    /// Filter by language.
    #[arg(short, long)]
    language: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl IssuesArgs {
    /// Run the search issues command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");
        q.push_str(" type:issue");

        if let Some(ref repo) = self.repo {
            let _ = write!(q, " repo:{repo}");
        }
        if let Some(ref state) = self.state {
            let _ = write!(q, " state:{state}");
        }
        if let Some(ref assignee) = self.assignee {
            let _ = write!(q, " assignee:{assignee}");
        }
        for label in &self.label {
            let _ = write!(q, " label:{label}");
        }
        if let Some(ref lang) = self.language {
            let _ = write!(q, " language:{lang}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=issues");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let encoded = ghc_core::text::percent_encode(&q);
        let path = format!("search/issues?q={encoded}&per_page={}", self.limit.min(100),);

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search issues")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let items = result
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected search response format"))?;

        if items.is_empty() {
            ios_eprintln!(ios, "No issues matched your search");
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in items {
            let number = item.get("number").and_then(Value::as_u64).unwrap_or(0);
            let title = item.get("title").and_then(Value::as_str).unwrap_or("");
            let state = item.get("state").and_then(Value::as_str).unwrap_or("");
            let repo_url = item
                .get("repository_url")
                .and_then(Value::as_str)
                .unwrap_or("");
            let repo_name = repo_url.rsplit('/').take(2).collect::<Vec<_>>();
            let repo_display = if repo_name.len() >= 2 {
                format!("{}/{}", repo_name[1], repo_name[0])
            } else {
                String::new()
            };

            let state_display = if state == "open" {
                cs.success("open")
            } else {
                cs.error("closed")
            };

            tp.add_row(vec![
                cs.bold(&repo_display),
                format!("#{number}"),
                text::truncate(title, 60),
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

    fn default_args(query: &str) -> IssuesArgs {
        IssuesArgs {
            query: vec![query.to_string()],
            limit: 30,
            repo: None,
            state: None,
            assignee: None,
            label: vec![],
            language: None,
            json: vec![],
            web: false,
        }
    }

    fn search_issues_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "number": 42,
                    "title": "Found Issue",
                    "state": "open",
                    "repository_url": "https://api.github.com/repos/owner/repo"
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_issues() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/issues", search_issues_response()).await;

        let args = default_args("bug fix");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("#42"), "should contain issue number");
        assert!(out.contains("Found Issue"), "should contain issue title");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("bug fix");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=issues"),
            "should open search URL with issue type"
        );
    }

    #[tokio::test]
    async fn test_should_show_empty_message_when_no_results() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/issues",
            serde_json::json!({ "total_count": 0, "items": [] }),
        )
        .await;

        let args = default_args("nonexistent");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("No issues matched"),
            "should show empty message"
        );
    }
}
