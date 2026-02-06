//! `ghc pr view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::text;

/// View a pull request.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Open in web browser.
    #[arg(short, long)]
    web: bool,

    /// Show comments.
    #[arg(short, long)]
    comments: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the pr view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the PR is not found.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/pull/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.number,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(ghc_api::queries::pr::PR_VIEW_QUERY, &variables)
            .await
            .context("failed to fetch pull request")?;

        let pr = data.pointer("/repository/pullRequest").ok_or_else(|| {
            anyhow::anyhow!(
                "pull request #{} not found in {}",
                self.number,
                repo.full_name(),
            )
        })?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // JSON output
        if !self.json.is_empty() {
            let json_output =
                serde_json::to_string_pretty(pr).context("failed to serialize JSON")?;
            ios_println!(ios, "{json_output}");
            return Ok(());
        }

        let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
        let body = pr
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("No description provided");
        let state = pr.get("state").and_then(Value::as_str).unwrap_or("OPEN");
        let is_draft = pr.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
        let author = pr
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let head_ref = pr.get("headRefName").and_then(Value::as_str).unwrap_or("");
        let base_ref = pr.get("baseRefName").and_then(Value::as_str).unwrap_or("");
        let url = pr.get("url").and_then(Value::as_str).unwrap_or("");
        let additions = pr.get("additions").and_then(Value::as_i64).unwrap_or(0);
        let deletions = pr.get("deletions").and_then(Value::as_i64).unwrap_or(0);
        let changed_files = pr.get("changedFiles").and_then(Value::as_i64).unwrap_or(0);
        let review_decision = pr
            .get("reviewDecision")
            .and_then(Value::as_str)
            .unwrap_or("");
        let comment_count = pr
            .pointer("/comments/totalCount")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        // Title and state
        let state_display = if is_draft {
            cs.gray("Draft")
        } else {
            match state {
                "OPEN" => cs.success("Open"),
                "CLOSED" => cs.error("Closed"),
                "MERGED" => cs.magenta("Merged"),
                _ => state.to_string(),
            }
        };

        ios_println!(
            ios,
            "{} #{} {}\n",
            cs.bold(title),
            self.number,
            state_display
        );
        ios_println!(
            ios,
            "{author} wants to merge into {base_ref} from {head_ref}\n"
        );

        // Labels
        if let Some(labels) = pr.pointer("/labels/nodes").and_then(Value::as_array)
            && !labels.is_empty()
        {
            let label_names: Vec<&str> = labels
                .iter()
                .filter_map(|l| l.get("name").and_then(Value::as_str))
                .collect();
            ios_println!(ios, "Labels: {}", label_names.join(", "));
        }

        // Review decision
        if !review_decision.is_empty() {
            let decision_display = match review_decision {
                "APPROVED" => cs.success("Approved"),
                "CHANGES_REQUESTED" => cs.warning("Changes requested"),
                "REVIEW_REQUIRED" => cs.gray("Review required"),
                _ => review_decision.to_string(),
            };
            ios_println!(ios, "Review: {decision_display}");
        }

        // Stats
        ios_println!(
            ios,
            "Changes: {} {} in {} {}",
            cs.success(&format!("+{additions}")),
            cs.error(&format!("-{deletions}")),
            changed_files,
            text::pluralize(changed_files, "file", "files"),
        );
        ios_println!(
            ios,
            "Comments: {}",
            text::pluralize(comment_count, "comment", "comments"),
        );

        // Body
        if !body.is_empty() {
            ios_println!(ios, "\n---");
            let rendered = ghc_core::markdown::render(body, ios.terminal_width());
            ios_println!(ios, "{rendered}");
        }

        ios_println!(ios, "\n{}", text::display_url(url));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    fn graphql_pr_view_response(pr: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": pr
                }
            }
        })
    }

    fn pr_view_fixture() -> serde_json::Value {
        serde_json::json!({
            "number": 42,
            "title": "Add logging",
            "body": "This PR adds structured logging.",
            "state": "OPEN",
            "isDraft": false,
            "author": { "login": "testuser" },
            "headRefName": "feature/logging",
            "baseRefName": "main",
            "labels": { "nodes": [{ "name": "enhancement", "color": "0075ca" }] },
            "url": "https://github.com/owner/repo/pull/42",
            "createdAt": "2024-01-15T10:00:00Z",
            "updatedAt": "2024-01-15T12:00:00Z",
            "comments": { "totalCount": 3 },
            "additions": 50,
            "deletions": 10,
            "changedFiles": 5,
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE"
        })
    }

    #[tokio::test]
    async fn test_should_view_pull_request() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(out.contains("Add logging"), "should contain title: {out}");
        assert!(out.contains("#42"), "should contain PR number: {out}");
        assert!(out.contains("main"), "should contain base ref: {out}");
        assert!(out.contains("+50"), "should contain additions: {out}");
    }

    #[tokio::test]
    async fn test_should_open_web_browser_for_pr_view() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: true,
            comments: false,
            json: vec![],
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/pull/42"));
    }

    #[tokio::test]
    async fn test_should_output_json_for_pr_view() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec!["number".into()],
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("\"number\": 42"),
            "should contain JSON number: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_pr_not_found() {
        let h = TestHarness::new().await;
        // When pullRequest is missing entirely (not null), pointer returns None
        let response = serde_json::json!({
            "data": {
                "repository": {}
            }
        });
        mock_graphql(&h.server, "PullRequestView", response).await;

        let args = ViewArgs {
            number: 999,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
