//! `ghc pr status` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::table::TablePrinter;
use ghc_core::text;

/// GraphQL query for pull request status overview.
const PR_STATUS_QUERY: &str = r"
query PullRequestStatus($owner: String!, $name: String!, $author: String!) {
  repository(owner: $owner, name: $name) {
    createdByUser: pullRequests(first: 10, states: OPEN, orderBy: {field: CREATED_AT, direction: DESC}, author: $author) {
      nodes {
        number
        title
        headRefName
        isDraft
        reviewDecision
        url
        createdAt
      }
    }
    reviewRequestedByUser: pullRequests(first: 10, states: OPEN, orderBy: {field: CREATED_AT, direction: DESC}) {
      nodes {
        number
        title
        headRefName
        isDraft
        reviewDecision
        reviewRequests(first: 10) {
          nodes {
            requestedReviewer {
              ... on User { login }
              ... on Team { name }
            }
          }
        }
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

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert("author".to_string(), Value::String(current_user.clone()));

        let data: Value = client
            .graphql(PR_STATUS_QUERY, &variables)
            .await
            .context("failed to fetch pull request status")?;

        // JSON output
        if !self.json.is_empty() {
            let json_output =
                serde_json::to_string_pretty(&data).context("failed to serialize JSON")?;
            ios_println!(ios, "{json_output}");
            return Ok(());
        }

        // Created by you
        let created = data
            .pointer("/repository/createdByUser/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "\n{}", cs.bold("Pull requests created by you"));
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

        // Review requested from you
        let review_requested = data
            .pointer("/repository/reviewRequestedByUser/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "\n{}", cs.bold("Pull requests requesting your review"));
        match review_requested {
            Some(prs) if !prs.is_empty() => {
                let mut tp = TablePrinter::new(ios);
                let mut found_any = false;

                for pr in prs {
                    // Filter to only PRs that actually request this user
                    let requests_this_user = pr
                        .pointer("/reviewRequests/nodes")
                        .and_then(Value::as_array)
                        .is_some_and(|nodes| {
                            nodes.iter().any(|node| {
                                node.pointer("/requestedReviewer/login")
                                    .and_then(Value::as_str)
                                    .is_some_and(|login| login.eq_ignore_ascii_case(&current_user))
                            })
                        });

                    if !requests_this_user {
                        continue;
                    }
                    found_any = true;

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

                if found_any {
                    ios_println!(ios, "{}", tp.render());
                } else {
                    ios_println!(ios, "  No pull requests requesting your review");
                }
            }
            _ => {
                println!("  No pull requests requesting your review");
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

        // Mock the PR status query
        mock_graphql(
            &h.server,
            "PullRequestStatus",
            serde_json::json!({
                "data": {
                    "repository": {
                        "createdByUser": {
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
                        "reviewRequestedByUser": {
                            "nodes": [{
                                "number": 51,
                                "title": "Review me",
                                "headRefName": "review-branch",
                                "isDraft": false,
                                "reviewDecision": null,
                                "reviewRequests": {
                                    "nodes": [{
                                        "requestedReviewer": {
                                            "login": "testuser"
                                        }
                                    }]
                                },
                                "url": "https://github.com/owner/repo/pull/51",
                                "createdAt": "2024-01-15T10:00:00Z"
                            }]
                        }
                    }
                }
            }),
        )
        .await;

        let args = StatusArgs {
            repo: "owner/repo".into(),
            json: vec![],
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("created by you"),
            "should show created section: {out}"
        );
        assert!(out.contains("#50"), "should contain created PR: {out}");
        assert!(
            out.contains("requesting your review"),
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
                        "createdByUser": { "nodes": [] },
                        "reviewRequestedByUser": { "nodes": [] }
                    }
                }
            }),
        )
        .await;

        let args = StatusArgs {
            repo: "owner/repo".into(),
            json: vec![],
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
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
