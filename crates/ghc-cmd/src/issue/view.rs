//! `ghc issue view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_print, ios_println};

/// View an issue.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Issue number.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Open the issue in the browser.
    #[arg(short, long)]
    web: bool,

    /// Show comments on the issue.
    #[arg(short, long)]
    comments: bool,

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

impl ViewArgs {
    /// Run the issue view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the API request
    /// fails, or the issue is not found.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/issues/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.number,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(ghc_api::queries::issue::ISSUE_VIEW_QUERY, &variables)
            .await
            .context("failed to fetch issue")?;

        let issue = data.pointer("/repository/issue").ok_or_else(|| {
            anyhow::anyhow!("issue #{} not found in {}", self.number, repo.full_name())
        })?;

        // JSON output with field filtering, jq, or template
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                issue,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        // Header
        let title = issue
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(no title)");
        let number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
        let state = issue.get("state").and_then(Value::as_str).unwrap_or("OPEN");

        let state_display = if state == "OPEN" {
            cs.success("Open")
        } else {
            cs.magenta("Closed")
        };

        ios_println!(ios, "{}", cs.bold(&format!("{title} #{number}")));
        ios_println!(ios, "{state_display}");

        // Author and timestamps
        let author = issue
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("ghost");
        let created_at = issue.get("createdAt").and_then(Value::as_str).unwrap_or("");

        ios_println!(
            ios,
            "{} opened this issue {}",
            cs.bold(author),
            cs.gray(created_at),
        );

        // Labels
        let labels: Vec<&str> = issue
            .pointer("/labels/nodes")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();

        if !labels.is_empty() {
            ios_println!(ios, "Labels: {}", labels.join(", "));
        }

        // Assignees
        let assignees: Vec<&str> = issue
            .pointer("/assignees/nodes")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("login").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();

        if !assignees.is_empty() {
            ios_println!(ios, "Assignees: {}", assignees.join(", "));
        }

        // Body
        let body = issue.get("body").and_then(Value::as_str).unwrap_or("");

        if body.is_empty() {
            ios_println!(ios, "\n{}", cs.gray("No description provided."));
        } else {
            ios_println!(ios);
            if ios.is_stdout_tty() {
                let rendered = ghc_core::markdown::render(body, ios.terminal_width());
                ios_print!(ios, "{rendered}");
            } else {
                ios_println!(ios, "{body}");
            }
        }

        // Comments summary
        let comment_count = issue
            .pointer("/comments/totalCount")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        if comment_count > 0 && !self.comments {
            ios_println!(
                ios,
                "\n{}",
                cs.gray(&format!(
                    "{} {}. Use --comments to view.",
                    comment_count,
                    text::pluralize(comment_count, "comment", "comments"),
                )),
            );
        }

        // URL
        let url = issue.get("url").and_then(Value::as_str).unwrap_or("");

        if !url.is_empty() {
            ios_println!(ios, "\n{}", text::display_url(url));
        }

        // Show comments if requested
        if self.comments && comment_count > 0 {
            self.print_comments(&client, &repo, ios, &cs).await?;
        }

        Ok(())
    }

    /// Fetch and print issue comments.
    async fn print_comments(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
    ) -> Result<()> {
        let path = format!(
            "repos/{}/{}/issues/{}/comments",
            repo.owner(),
            repo.name(),
            self.number,
        );

        let comments: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch comments")?;

        ios_println!(ios, "\n{}", cs.bold("Comments:"));
        ios_println!(ios, "{}", "-".repeat(40));

        for comment in &comments {
            let author = comment
                .pointer("/user/login")
                .and_then(Value::as_str)
                .unwrap_or("ghost");
            let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
            let created_at = comment
                .get("created_at")
                .and_then(Value::as_str)
                .unwrap_or("");

            ios_println!(
                ios,
                "\n{} commented {}",
                cs.bold(author),
                cs.gray(created_at),
            );
            ios_println!(ios, "{body}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    fn view_issue_response(number: i64, title: &str, state: &str, body: &str) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "issue": {
                        "number": number,
                        "title": title,
                        "state": state,
                        "body": body,
                        "url": format!("https://github.com/owner/repo/issues/{number}"),
                        "author": { "login": "testuser" },
                        "labels": { "nodes": [] },
                        "assignees": { "nodes": [] },
                        "comments": { "totalCount": 0 },
                        "createdAt": "2024-01-15T10:00:00Z",
                        "updatedAt": "2024-01-15T10:00:00Z"
                    }
                }
            }
        })
    }

    fn default_args(number: i32, repo: &str) -> ViewArgs {
        ViewArgs {
            number,
            repo: repo.to_string(),
            web: false,
            comments: false,
            json: vec![],
            jq: None,
            template: None,
        }
    }

    #[tokio::test]
    async fn test_should_view_issue() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "repository",
            view_issue_response(42, "Test Issue", "OPEN", "Issue body text"),
        )
        .await;

        let args = default_args(42, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("Test Issue"), "should contain issue title");
        assert!(out.contains("#42"), "should contain issue number");
        assert!(out.contains("Issue body text"), "should contain issue body");
    }

    #[tokio::test]
    async fn test_should_output_json_when_requested() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "repository",
            view_issue_response(1, "JSON view", "OPEN", "body"),
        )
        .await;

        let mut args = default_args(1, "owner/repo");
        args.json = vec!["title".to_string()];
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("JSON view"),
            "should contain issue title in JSON"
        );
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args(42, "owner/repo");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("/issues/42"),
            "should open correct issue URL"
        );
    }
}
