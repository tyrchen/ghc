//! `ghc search commits` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Search for commits across GitHub.
#[derive(Debug, Args)]
pub struct CommitsArgs {
    /// Search query.
    #[arg(value_name = "QUERY", required = true)]
    query: Vec<String>,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Filter by author.
    #[arg(long)]
    author: Option<String>,

    /// Filter by committer.
    #[arg(long)]
    committer: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl CommitsArgs {
    /// Run the search commits command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");

        if let Some(ref repo) = self.repo {
            let _ = write!(q, " repo:{repo}");
        }
        if let Some(ref author) = self.author {
            let _ = write!(q, " author:{author}");
        }
        if let Some(ref committer) = self.committer {
            let _ = write!(q, " committer:{committer}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=commits");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let encoded = ghc_core::text::percent_encode(&q);
        let path = format!(
            "search/commits?q={encoded}&per_page={}",
            self.limit.min(100),
        );

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search commits")?;

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
            ios_eprintln!(ios, "No commits matched your search");
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in items {
            let sha = item.get("sha").and_then(Value::as_str).unwrap_or("");
            let short_sha = if sha.len() >= 7 { &sha[..7] } else { sha };
            let message = item
                .pointer("/commit/message")
                .and_then(Value::as_str)
                .unwrap_or("");
            let first_line = message.lines().next().unwrap_or("");
            let repo_name = item
                .pointer("/repository/full_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let date = item
                .pointer("/commit/author/date")
                .and_then(Value::as_str)
                .unwrap_or("");

            tp.add_row(vec![
                cs.bold(repo_name),
                cs.cyan(short_sha),
                text::truncate(first_line, 60),
                date.to_string(),
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

    fn default_args(query: &str) -> CommitsArgs {
        CommitsArgs {
            query: vec![query.to_string()],
            limit: 30,
            repo: None,
            author: None,
            committer: None,
            json: vec![],
            web: false,
        }
    }

    fn search_commits_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "sha": "abc1234567890",
                    "commit": {
                        "message": "Fix critical bug\n\nDetailed description",
                        "author": { "date": "2024-01-15T10:00:00Z" }
                    },
                    "repository": { "full_name": "owner/repo" }
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_commits() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/commits", search_commits_response()).await;

        let args = default_args("fix bug");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("abc1234"), "should contain short SHA");
        assert!(
            out.contains("Fix critical bug"),
            "should contain commit message first line"
        );
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("fix bug");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=commits"),
            "should open search URL with commits type"
        );
    }

    #[tokio::test]
    async fn test_should_show_empty_message_when_no_results() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/commits",
            serde_json::json!({ "total_count": 0, "items": [] }),
        )
        .await;

        let args = default_args("nonexistent");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("No commits matched"),
            "should show empty message"
        );
    }
}
