//! `ghc pr list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// List pull requests in a repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Filter by state.
    #[arg(short, long, default_value = "open", value_parser = ["open", "closed", "merged", "all"])]
    state: String,

    /// Maximum number of pull requests to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by head branch name.
    #[arg(short = 'H', long)]
    head: Option<String>,

    /// Filter by base branch name.
    #[arg(short = 'B', long)]
    base: Option<String>,

    /// Filter by label.
    #[arg(short, long)]
    label: Vec<String>,

    /// Filter by author.
    #[arg(short = 'A', long)]
    author: Option<String>,

    /// Filter by assignee.
    #[arg(long)]
    assignee: Option<String>,

    /// Include draft pull requests.
    #[arg(long)]
    draft: bool,

    /// Open in web browser.
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
}

impl ListArgs {
    /// Run the pr list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the response is malformed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/pulls",
                repo.host(),
                repo.owner(),
                repo.name(),
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let states = match self.state.as_str() {
            "closed" => vec![Value::String("CLOSED".to_string())],
            "merged" => vec![Value::String("MERGED".to_string())],
            "all" => vec![
                Value::String("OPEN".to_string()),
                Value::String("CLOSED".to_string()),
                Value::String("MERGED".to_string()),
            ],
            _ => vec![Value::String("OPEN".to_string())],
        };

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(self.limit.min(100))),
        );
        variables.insert("states".to_string(), Value::Array(states));

        if let Some(ref head) = self.head {
            variables.insert("headRefName".to_string(), Value::String(head.clone()));
        }
        if let Some(ref base) = self.base {
            variables.insert("baseRefName".to_string(), Value::String(base.clone()));
        }
        if !self.label.is_empty() {
            let labels: Vec<Value> = self
                .label
                .iter()
                .map(|l| Value::String(l.clone()))
                .collect();
            variables.insert("labels".to_string(), Value::Array(labels));
        }

        let data: Value = client
            .graphql(ghc_api::queries::pr::PR_LIST_QUERY, &variables)
            .await
            .context("failed to list pull requests")?;

        let prs = data
            .pointer("/repository/pullRequests/nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected API response format"))?;

        let ios = &factory.io;

        // JSON output mode with field filtering, jq, or template
        // Always produces output (even [] for empty results)
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let mut arr = Value::Array(prs.clone());
            ghc_core::json::normalize_graphql_connections(&mut arr);
            ghc_core::json::normalize_author(&mut arr);
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if prs.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(
                    ios,
                    "No pull requests match your search in {}",
                    repo.full_name()
                );
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for pr in prs {
            let number = pr.get("number").and_then(Value::as_i64).unwrap_or(0);
            let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
            let state = pr.get("state").and_then(Value::as_str).unwrap_or("OPEN");
            let is_draft = pr.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
            let author = pr
                .pointer("/author/login")
                .and_then(Value::as_str)
                .unwrap_or("");
            let head_ref = pr.get("headRefName").and_then(Value::as_str).unwrap_or("");
            let created_at = pr.get("createdAt").and_then(Value::as_str).unwrap_or("");

            // Filter by author if specified
            if let Some(ref filter_author) = self.author
                && !author.eq_ignore_ascii_case(filter_author)
            {
                continue;
            }

            // Filter drafts unless --draft is set
            if !self.draft && is_draft {
                continue;
            }

            let state_display = if is_draft {
                cs.gray("DRAFT")
            } else {
                match state {
                    "OPEN" => cs.success("OPEN"),
                    "CLOSED" => cs.error("CLOSED"),
                    "MERGED" => cs.magenta("MERGED"),
                    _ => state.to_string(),
                }
            };

            let time_display = chrono::DateTime::parse_from_rfc3339(created_at).map_or_else(
                |_| created_at.to_string(),
                |dt| text::relative_time_str(&dt.into(), ios.is_stdout_tty()),
            );

            tp.add_row(vec![
                cs.bold(&format!("#{number}")),
                text::truncate(title, 60),
                head_ref.to_string(),
                state_display,
                author.to_string(),
                time_display,
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
    use crate::test_helpers::{TestHarness, graphql_pr_list_response, mock_graphql, pr_fixture};

    #[tokio::test]
    async fn test_should_list_open_pull_requests() {
        let h = TestHarness::new().await;
        let prs = vec![
            pr_fixture(1, "Fix bug", "OPEN"),
            pr_fixture(2, "Add feature", "OPEN"),
        ];
        mock_graphql(&h.server, "PullRequestList", graphql_pr_list_response(&prs)).await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            state: "open".into(),
            limit: 30,
            head: None,
            base: None,
            label: vec![],
            author: None,
            assignee: None,
            draft: false,
            web: false,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(out.contains("#1"), "should contain PR #1: {out}");
        assert!(out.contains("#2"), "should contain PR #2: {out}");
        assert!(out.contains("Fix bug"), "should contain title: {out}");
    }

    #[tokio::test]
    async fn test_should_open_web_browser_for_pr_list() {
        let h = TestHarness::new().await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            state: "open".into(),
            limit: 30,
            head: None,
            base: None,
            label: vec![],
            author: None,
            assignee: None,
            draft: false,
            web: true,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/pulls"));
    }

    #[tokio::test]
    async fn test_should_output_json_for_pr_list() {
        let h = TestHarness::new().await;
        let prs = vec![pr_fixture(5, "JSON test", "OPEN")];
        mock_graphql(&h.server, "PullRequestList", graphql_pr_list_response(&prs)).await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            state: "open".into(),
            limit: 30,
            head: None,
            base: None,
            label: vec![],
            author: None,
            assignee: None,
            draft: false,
            web: false,
            json: vec!["number".into()],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("\"number\":5"),
            "should contain JSON number: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_format() {
        let h = TestHarness::new().await;
        let args = ListArgs {
            repo: "invalid-repo".into(),
            state: "open".into(),
            limit: 30,
            head: None,
            base: None,
            label: vec![],
            author: None,
            assignee: None,
            draft: false,
            web: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid repository"),
        );
    }
}
