//! `ghc pr status` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::table::TablePrinter;
use ghc_core::text;

/// GraphQL query for pull request status overview.
///
/// Uses `search` API for "created by user" and "review requested" sections,
/// matching the Go CLI approach. The `pullRequests` connection on Repository
/// does not accept an `author` argument.
const PR_STATUS_QUERY: &str = r"
query PullRequestStatus($owner: String!, $name: String!, $headRefName: String!, $viewerQuery: String!, $reviewerQuery: String!) {
  repository(owner: $owner, name: $name) {
    pullRequests(headRefName: $headRefName, first: 1, states: [OPEN, CLOSED, MERGED], orderBy: {field: CREATED_AT, direction: DESC}) {
      nodes {
        number
        title
        state
        headRefName
        isDraft
        reviewDecision
        url
        createdAt
      }
    }
  }
  viewerCreated: search(query: $viewerQuery, type: ISSUE, first: 10) {
    nodes {
      ... on PullRequest {
        number
        title
        headRefName
        isDraft
        reviewDecision
        url
        createdAt
      }
    }
  }
  reviewRequested: search(query: $reviewerQuery, type: ISSUE, first: 10) {
    nodes {
      ... on PullRequest {
        number
        title
        headRefName
        isDraft
        reviewDecision
        url
        createdAt
      }
    }
  }
}
";

/// Show the status of pull requests relevant to you.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

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

impl StatusArgs {
    /// Run the pr status command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Get current user
        let current_user = client
            .current_login()
            .await
            .context("failed to get authenticated user")?;

        // Try to get the current git branch for the "Current branch" section
        let head_ref_name = match factory.git_client() {
            Ok(gc) => gc.current_branch().await.unwrap_or_default(),
            Err(_) => String::new(),
        };

        let full_name = format!("{}/{}", repo.owner(), repo.name());
        let viewer_query = format!("repo:{full_name} state:open is:pr author:{current_user}");
        let reviewer_query =
            format!("repo:{full_name} state:open is:pr review-requested:{current_user}");

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "headRefName".to_string(),
            Value::String(head_ref_name.clone()),
        );
        variables.insert("viewerQuery".to_string(), Value::String(viewer_query));
        variables.insert("reviewerQuery".to_string(), Value::String(reviewer_query));

        let data: Value = client
            .graphql(PR_STATUS_QUERY, &variables)
            .await
            .context("failed to fetch pull request status")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &data,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        // Current branch section
        ios_println!(ios, "\n{}", cs.bold("Current branch"));
        if head_ref_name.is_empty() {
            ios_println!(ios, "  There is no current branch");
        } else {
            let current_branch_pr = data
                .pointer("/repository/pullRequests/nodes")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first());

            match current_branch_pr {
                Some(pr) => {
                    let number = pr.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
                    let state = pr.get("state").and_then(Value::as_str).unwrap_or("OPEN");
                    let is_draft = pr.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
                    let review_decision = pr
                        .get("reviewDecision")
                        .and_then(Value::as_str)
                        .unwrap_or("");

                    let status_icon = if is_draft {
                        cs.gray("o")
                    } else {
                        match state {
                            "MERGED" => cs.magenta("*"),
                            "CLOSED" => cs.error("x"),
                            _ => match review_decision {
                                "APPROVED" => cs.success_icon(),
                                "CHANGES_REQUESTED" => cs.warning_icon(),
                                _ => cs.success("o"),
                            },
                        }
                    };

                    let mut tp = TablePrinter::new(ios);
                    tp.add_row(vec![
                        status_icon,
                        cs.bold(&format!("#{number}")),
                        text::truncate(title, 50),
                        cs.gray(&head_ref_name),
                    ]);
                    ios_println!(ios, "{}", tp.render());
                }
                None => {
                    ios_println!(
                        ios,
                        "  There is no pull request associated with {}",
                        cs.bold(&format!("[{head_ref_name}]")),
                    );
                }
            }
        }

        // Created by you
        let created = data
            .pointer("/viewerCreated/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "\n{}", cs.bold("Created by you"));
        match created {
            Some(prs) if !prs.is_empty() => {
                let mut tp = TablePrinter::new(ios);
                for pr in prs {
                    let number = pr.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
                    let head_ref = pr.get("headRefName").and_then(Value::as_str).unwrap_or("");
                    let is_draft = pr.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
                    let review_decision = pr
                        .get("reviewDecision")
                        .and_then(Value::as_str)
                        .unwrap_or("");

                    let status_icon = if is_draft {
                        cs.gray("o")
                    } else {
                        match review_decision {
                            "APPROVED" => cs.success_icon(),
                            "CHANGES_REQUESTED" => cs.warning_icon(),
                            _ => cs.success("o"),
                        }
                    };

                    tp.add_row(vec![
                        status_icon,
                        cs.bold(&format!("#{number}")),
                        text::truncate(title, 50),
                        cs.gray(head_ref),
                    ]);
                }
                ios_println!(ios, "{}", tp.render());
            }
            _ => {
                ios_println!(ios, "  You have no open pull requests");
            }
        }

        // Review requested from you (already filtered by the search query)
        let review_requested = data
            .pointer("/reviewRequested/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "\n{}", cs.bold("Requesting a code review from you"));
        match review_requested {
            Some(prs) if !prs.is_empty() => {
                let mut tp = TablePrinter::new(ios);

                for pr in prs {
                    let number = pr.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
                    let head_ref = pr.get("headRefName").and_then(Value::as_str).unwrap_or("");

                    tp.add_row(vec![
                        cs.warning("!"),
                        cs.bold(&format!("#{number}")),
                        text::truncate(title, 50),
                        cs.gray(head_ref),
                    ]);
                }

                ios_println!(ios, "{}", tp.render());
            }
            _ => {
                ios_println!(ios, "  No pull requests requesting your review");
            }
        }

        ios_println!(ios);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_show_pr_status() {
        let h = TestHarness::new().await;

        // Mock the current_login query
        mock_graphql(
            &h.server,
            "UserCurrent",
            serde_json::json!({
                "data": {
                    "viewer": { "login": "testuser" }
                }
            }),
        )
        .await;

        // Mock the PR status query (uses search API + repository for current branch)
        mock_graphql(
            &h.server,
            "PullRequestStatus",
            serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequests": {
                            "nodes": []
                        }
                    },
                    "viewerCreated": {
                        "nodes": [{
                            "number": 50,
                            "title": "My PR",
                            "headRefName": "my-branch",
                            "isDraft": false,
                            "reviewDecision": "APPROVED",
                            "url": "https://github.com/owner/repo/pull/50",
                            "createdAt": "2024-01-15T10:00:00Z"
                        }]
                    },
                    "reviewRequested": {
                        "nodes": [{
                            "number": 51,
                            "title": "Review me",
                            "headRefName": "review-branch",
                            "isDraft": false,
                            "reviewDecision": null,
                            "url": "https://github.com/owner/repo/pull/51",
                            "createdAt": "2024-01-15T10:00:00Z"
                        }]
                    }
                }
            }),
        )
        .await;

        let args = StatusArgs {
            repo: "owner/repo".into(),
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("Current branch"),
            "should show current branch section: {out}"
        );
        assert!(
            out.contains("Created by you"),
            "should show created section: {out}"
        );
        assert!(out.contains("#50"), "should contain created PR: {out}");
        assert!(
            out.contains("Requesting a code review from you"),
            "should show review section: {out}",
        );
        assert!(out.contains("#51"), "should contain review PR: {out}");
    }

    #[tokio::test]
    async fn test_should_show_empty_status() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "UserCurrent",
            serde_json::json!({
                "data": { "viewer": { "login": "testuser" } }
            }),
        )
        .await;

        mock_graphql(
            &h.server,
            "PullRequestStatus",
            serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequests": { "nodes": [] }
                    },
                    "viewerCreated": { "nodes": [] },
                    "reviewRequested": { "nodes": [] }
                }
            }),
        )
        .await;

        let args = StatusArgs {
            repo: "owner/repo".into(),
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("no open pull requests"),
            "should show no PRs: {out}",
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_status() {
        let h = TestHarness::new().await;
        let args = StatusArgs {
            repo: "bad".into(),
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
