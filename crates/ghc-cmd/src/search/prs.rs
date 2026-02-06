//! `ghc search prs` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Search for pull requests across GitHub.
#[derive(Debug, Args)]
pub struct PrsArgs {
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
    #[arg(long, value_parser = ["open", "closed", "merged"])]
    state: Option<String>,

    /// Filter by author.
    #[arg(long)]
    author: Option<String>,

    /// Filter by assignee.
    #[arg(long)]
    assignee: Option<String>,

    /// Filter by label.
    #[arg(long)]
    label: Vec<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl PrsArgs {
    /// Run the search prs command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");
        q.push_str(" type:pr");

        if let Some(ref repo) = self.repo {
            let _ = write!(q, " repo:{repo}");
        }
        if let Some(ref state) = self.state {
            if state == "merged" {
                q.push_str(" is:merged");
            } else {
                let _ = write!(q, " state:{state}");
            }
        }
        if let Some(ref author) = self.author {
            let _ = write!(q, " author:{author}");
        }
        if let Some(ref assignee) = self.assignee {
            let _ = write!(q, " assignee:{assignee}");
        }
        for label in &self.label {
            let _ = write!(q, " label:{label}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=pullrequests");
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
            .context("failed to search pull requests")?;

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
            ios_eprintln!(ios, "No pull requests matched your search");
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

            let is_merged = item
                .get("pull_request")
                .and_then(|pr| pr.get("merged_at"))
                .is_some_and(|m| !m.is_null());

            let state_display = if is_merged {
                cs.magenta("merged")
            } else if state == "open" {
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

    fn default_args(query: &str) -> PrsArgs {
        PrsArgs {
            query: vec![query.to_string()],
            limit: 30,
            repo: None,
            state: None,
            author: None,
            assignee: None,
            label: vec![],
            json: vec![],
            web: false,
        }
    }

    fn search_prs_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "number": 55,
                    "title": "Found PR",
                    "state": "open",
                    "repository_url": "https://api.github.com/repos/owner/repo",
                    "pull_request": { "merged_at": null }
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_prs() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/issues", search_prs_response()).await;

        let args = default_args("feature");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("#55"), "should contain PR number");
        assert!(out.contains("Found PR"), "should contain PR title");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("feature");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=pullrequests"),
            "should open search URL with PR type"
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
            err.contains("No pull requests matched"),
            "should show empty message"
        );
    }
}
