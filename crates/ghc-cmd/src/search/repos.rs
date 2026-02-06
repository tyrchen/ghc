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

    /// Filter on repository owner.
    #[arg(long)]
    owner: Vec<String>,

    /// Filter based on created at date.
    #[arg(long)]
    created: Option<String>,

    /// Filter based on number of followers.
    #[arg(long)]
    followers: Option<String>,

    /// Include forks in fetched repositories (false, true, or only).
    #[arg(long, value_parser = ["false", "true", "only"])]
    include_forks: Option<String>,

    /// Filter on number of forks.
    #[arg(long)]
    forks: Option<String>,

    /// Filter on number of issues with the 'good first issue' label.
    #[arg(long)]
    good_first_issues: Option<String>,

    /// Filter on number of issues with the 'help wanted' label.
    #[arg(long)]
    help_wanted_issues: Option<String>,

    /// Restrict search to specific field of repository.
    #[arg(long, value_parser = ["name", "description", "readme"])]
    r#match: Vec<String>,

    /// Filter based on license type.
    #[arg(long)]
    license: Vec<String>,

    /// Filter on last updated at date.
    #[arg(long)]
    updated: Option<String>,

    /// Filter on a size range, in kilobytes.
    #[arg(long)]
    size: Option<String>,

    /// Filter on number of stars.
    #[arg(long)]
    stars: Option<String>,

    /// Filter on number of topics.
    #[arg(long)]
    number_topics: Option<String>,

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

impl ReposArgs {
    /// Run the search repos command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    #[allow(clippy::too_many_lines)]
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
        for owner in &self.owner {
            let _ = write!(q, " user:{owner}");
        }
        if let Some(ref created) = self.created {
            let _ = write!(q, " created:{created}");
        }
        if let Some(ref followers) = self.followers {
            let _ = write!(q, " followers:{followers}");
        }
        if let Some(ref include_forks) = self.include_forks {
            let _ = write!(q, " fork:{include_forks}");
        }
        if let Some(ref forks) = self.forks {
            let _ = write!(q, " forks:{forks}");
        }
        if let Some(ref gfi) = self.good_first_issues {
            let _ = write!(q, " good-first-issues:{gfi}");
        }
        if let Some(ref hwi) = self.help_wanted_issues {
            let _ = write!(q, " help-wanted-issues:{hwi}");
        }
        for m in &self.r#match {
            let _ = write!(q, " in:{m}");
        }
        for lic in &self.license {
            let _ = write!(q, " license:{lic}");
        }
        if let Some(ref updated) = self.updated {
            let _ = write!(q, " pushed:{updated}");
        }
        if let Some(ref size) = self.size {
            let _ = write!(q, " size:{size}");
        }
        if let Some(ref stars) = self.stars {
            let _ = write!(q, " stars:{stars}");
        }
        if let Some(ref nt) = self.number_topics {
            let _ = write!(q, " topics:{nt}");
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
            owner: vec![],
            created: None,
            followers: None,
            include_forks: None,
            forks: None,
            good_first_issues: None,
            help_wanted_issues: None,
            r#match: vec![],
            license: vec![],
            updated: None,
            size: None,
            stars: None,
            number_topics: None,
            json: vec![],
            jq: None,
            template: None,
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
