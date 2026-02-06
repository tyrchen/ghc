//! Status command (`ghc status`).
//!
//! Show status of relevant work across GitHub.

use std::collections::HashMap;
use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Show status of relevant issues, pull requests, and notifications.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Exclude notifications from an organization.
    #[arg(short, long)]
    exclude: Vec<String>,

    /// Only show items from a specific organization.
    #[arg(short, long)]
    org: Option<String>,
}

impl StatusArgs {
    /// Run the status command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Fetch assigned issues
        let assigned_query = r"
            query AssignedSearch($query: String!) {
              search(query: $query, type: ISSUE, first: 25) {
                nodes {
                  ... on Issue {
                    __typename
                    number
                    title
                    url
                    repository { nameWithOwner }
                    updatedAt
                  }
                  ... on PullRequest {
                    __typename
                    number
                    title
                    url
                    repository { nameWithOwner }
                    updatedAt
                  }
                }
              }
            }
        ";

        let mut search_query = "assignee:@me is:open".to_string();
        if let Some(ref org) = self.org {
            let _ = write!(search_query, " org:{org}");
        }
        for excluded in &self.exclude {
            let _ = write!(search_query, " -org:{excluded}");
        }

        let mut variables = HashMap::new();
        variables.insert("query".to_string(), Value::String(search_query));

        let assigned_data: Value = client
            .graphql(assigned_query, &variables)
            .await
            .context("failed to fetch assigned items")?;

        // Fetch review requests
        let mut review_query = "review-requested:@me is:open is:pr".to_string();
        if let Some(ref org) = self.org {
            let _ = write!(review_query, " org:{org}");
        }
        for excluded in &self.exclude {
            let _ = write!(review_query, " -org:{excluded}");
        }

        let mut review_variables = HashMap::new();
        review_variables.insert("query".to_string(), Value::String(review_query));

        let review_data: Value = client
            .graphql(assigned_query, &review_variables)
            .await
            .context("failed to fetch review requests")?;

        // Display assigned items
        let assigned_nodes = assigned_data
            .pointer("/search/nodes")
            .and_then(Value::as_array);

        ios_eprintln!(ios, "{}", cs.bold("Assigned Issues and Pull Requests"));
        if let Some(nodes) = assigned_nodes {
            if nodes.is_empty() {
                ios_eprintln!(ios, "  Nothing assigned to you");
            } else {
                for node in nodes {
                    let typename = node
                        .get("__typename")
                        .and_then(Value::as_str)
                        .unwrap_or("Issue");
                    let repo_name = node
                        .pointer("/repository/nameWithOwner")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let number = node.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = node.get("title").and_then(Value::as_str).unwrap_or("");

                    let icon = if typename == "PullRequest" {
                        "PR"
                    } else {
                        "Issue"
                    };
                    ios_eprintln!(ios, "  {icon} {repo_name}#{number} {title}");
                }
            }
        } else {
            ios_eprintln!(ios, "  Nothing assigned to you");
        }

        // Display review requests
        let review_nodes = review_data
            .pointer("/search/nodes")
            .and_then(Value::as_array);

        ios_eprintln!(ios, "\n{}", cs.bold("Review Requests"));
        if let Some(nodes) = review_nodes {
            if nodes.is_empty() {
                ios_eprintln!(ios, "  No review requests");
            } else {
                for node in nodes {
                    let repo_name = node
                        .pointer("/repository/nameWithOwner")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let number = node.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = node.get("title").and_then(Value::as_str).unwrap_or("");

                    ios_eprintln!(ios, "  PR {repo_name}#{number} {title}");
                }
            }
        } else {
            ios_eprintln!(ios, "  No review requests");
        }

        // Fetch notifications
        let notifications: Vec<Value> = client
            .rest(
                reqwest::Method::GET,
                "notifications?per_page=10",
                None::<&Value>,
            )
            .await
            .context("failed to fetch notifications")?;

        ios_eprintln!(ios, "\n{}", cs.bold("Notifications"));
        if notifications.is_empty() {
            ios_eprintln!(ios, "  No unread notifications");
        } else {
            for notif in notifications.iter().take(10) {
                let reason = notif
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let title = notif
                    .pointer("/subject/title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let repo_name = notif
                    .pointer("/repository/full_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                ios_eprintln!(ios, "  [{reason}] {repo_name}: {title}");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql, mock_rest_get};

    #[tokio::test]
    async fn test_should_display_empty_status() {
        let h = TestHarness::new().await;

        // Mock GraphQL search (assigned items)
        mock_graphql(
            &h.server,
            "AssignedSearch",
            serde_json::json!({
                "data": {
                    "search": {
                        "nodes": []
                    }
                }
            }),
        )
        .await;

        // Mock notifications REST endpoint
        mock_rest_get(&h.server, "/notifications", serde_json::json!([])).await;

        let args = StatusArgs {
            exclude: vec![],
            org: None,
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(stderr.contains("Assigned Issues and Pull Requests"));
        assert!(stderr.contains("Nothing assigned to you"));
        assert!(stderr.contains("Review Requests"));
        assert!(stderr.contains("No review requests"));
        assert!(stderr.contains("Notifications"));
        assert!(stderr.contains("No unread notifications"));
    }

    #[tokio::test]
    async fn test_should_display_assigned_items() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "AssignedSearch",
            serde_json::json!({
                "data": {
                    "search": {
                        "nodes": [
                            {
                                "__typename": "Issue",
                                "number": 42,
                                "title": "Fix bug",
                                "url": "https://github.com/owner/repo/issues/42",
                                "repository": { "nameWithOwner": "owner/repo" },
                                "updatedAt": "2024-01-15T10:00:00Z"
                            }
                        ]
                    }
                }
            }),
        )
        .await;

        mock_rest_get(&h.server, "/notifications", serde_json::json!([])).await;

        let args = StatusArgs {
            exclude: vec![],
            org: None,
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(stderr.contains("Issue owner/repo#42 Fix bug"));
    }
}
