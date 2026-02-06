//! `ghc search repos` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Search for repositories across GitHub.
#[derive(Debug, Args)]
pub struct ReposArgs {
    /// Search query.
    #[arg(value_name = "QUERY", required = true)]
    query: Vec<String>,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by language.
    #[arg(short, long)]
    language: Option<String>,

    /// Filter by topic.
    #[arg(long)]
    topic: Vec<String>,

    /// Filter by visibility.
    #[arg(long, value_parser = ["public", "private"])]
    visibility: Option<String>,

    /// Sort results.
    #[arg(long, value_parser = ["stars", "forks", "help-wanted-issues", "updated"])]
    sort: Option<String>,

    /// Sort order.
    #[arg(long, value_parser = ["asc", "desc"], default_value = "desc")]
    order: String,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl ReposArgs {
    /// Run the search repos command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");

        if let Some(ref lang) = self.language {
            let _ = write!(q, " language:{lang}");
        }
        for topic in &self.topic {
            let _ = write!(q, " topic:{topic}");
        }
        if let Some(ref vis) = self.visibility {
            let _ = write!(q, " is:{vis}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=repositories");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let encoded = ghc_core::text::percent_encode(&q);
        let mut path = format!(
            "search/repositories?q={encoded}&per_page={}",
            self.limit.min(100),
        );
        if let Some(ref sort) = self.sort {
            let _ = write!(path, "&sort={sort}&order={}", self.order);
        }

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search repositories")?;

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
            ios_eprintln!(ios, "No repositories matched your search");
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in items {
            let full_name = item.get("full_name").and_then(Value::as_str).unwrap_or("");
            let description = item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let stars = item
                .get("stargazers_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let language = item.get("language").and_then(Value::as_str).unwrap_or("");
            let is_private = item
                .get("private")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let updated_at = item.get("updated_at").and_then(Value::as_str).unwrap_or("");

            let visibility = if is_private {
                cs.warning("private")
            } else {
                cs.success("public")
            };

            tp.add_row(vec![
                cs.bold(full_name),
                text::truncate(description, 50),
                visibility,
                language.to_string(),
                format!("{stars}"),
                updated_at.to_string(),
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

    fn default_args(query: &str) -> ReposArgs {
        ReposArgs {
            query: vec![query.to_string()],
            limit: 30,
            language: None,
            topic: vec![],
            visibility: None,
            sort: None,
            order: "desc".to_string(),
            json: vec![],
            web: false,
        }
    }

    fn search_repos_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "full_name": "owner/my-repo",
                    "description": "A test repo",
                    "stargazers_count": 100,
                    "language": "Rust",
                    "private": false,
                    "updated_at": "2024-01-15T10:00:00Z"
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_repos() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/repositories", search_repos_response()).await;

        let args = default_args("rust cli");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("owner/my-repo"), "should contain repo name");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("rust cli");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=repositories"),
            "should open search URL with type"
        );
    }

    #[tokio::test]
    async fn test_should_show_empty_message_when_no_results() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/repositories",
            serde_json::json!({ "total_count": 0, "items": [] }),
        )
        .await;

        let args = default_args("nonexistent");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("No repositories matched"),
            "should show empty message"
        );
    }
}
