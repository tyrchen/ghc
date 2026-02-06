//! `ghc issue status` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::table::TablePrinter;
use ghc_core::text;

/// Show status of relevant issues.
///
/// Displays issues assigned to the authenticated user, issues mentioning
/// the authenticated user, and recently opened issues in the repository.
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
    /// Run the issue status command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the API request
    /// fails, or the authenticated user cannot be determined.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Get the current user login
        let current_login = client
            .current_login()
            .await
            .context("failed to determine authenticated user")?;

        let query = r"
            query IssueStatus(
              $owner: String!,
              $name: String!,
              $assignee: String!,
              $mention: String!
            ) {
              assigned: repository(owner: $owner, name: $name) {
                issues(first: 25, states: [OPEN], filterBy: {assignee: $assignee}, orderBy: {field: UPDATED_AT, direction: DESC}) {
                  nodes {
                    number
                    title
                    url
                    updatedAt
                  }
                }
              }
              mentioned: repository(owner: $owner, name: $name) {
                issues(first: 25, states: [OPEN], filterBy: {mentioned: $mention}, orderBy: {field: UPDATED_AT, direction: DESC}) {
                  nodes {
                    number
                    title
                    url
                    updatedAt
                  }
                }
              }
              recent: repository(owner: $owner, name: $name) {
                issues(first: 25, states: [OPEN], orderBy: {field: CREATED_AT, direction: DESC}) {
                  nodes {
                    number
                    title
                    author { login }
                    url
                    createdAt
                  }
                }
              }
            }
        ";

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert("assignee".to_string(), Value::String(current_login.clone()));
        variables.insert("mention".to_string(), Value::String(current_login.clone()));

        let data: Value = client
            .graphql(query, &variables)
            .await
            .context("failed to fetch issue status")?;

        // JSON output
        if !self.json.is_empty() {
            let json_output =
                serde_json::to_string_pretty(&data).context("failed to serialize JSON")?;
            ios_println!(ios, "{json_output}");
            return Ok(());
        }

        // Assigned issues
        let assigned_issues = data
            .pointer("/assigned/issues/nodes")
            .and_then(Value::as_array);

        ios_println!(
            ios,
            "{}",
            cs.bold(&format!("Issues assigned to you in {}", repo.full_name())),
        );

        if let Some(issues) = assigned_issues {
            if issues.is_empty() {
                ios_println!(ios, "  {}", cs.gray("Nothing here"));
            } else {
                let mut tp = TablePrinter::new(ios);
                for issue in issues {
                    let number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = issue.get("title").and_then(Value::as_str).unwrap_or("");
                    tp.add_row(vec![format!("  #{number}"), text::truncate(title, 60)]);
                }
                ios_println!(ios, "{}", tp.render());
            }
        } else {
            ios_println!(ios, "  {}", cs.gray("Nothing here"));
        }

        ios_println!(ios);

        // Mentioning issues
        let mentioned_issues = data
            .pointer("/mentioned/issues/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "{}", cs.bold("Issues mentioning you"));

        if let Some(issues) = mentioned_issues {
            if issues.is_empty() {
                ios_println!(ios, "  {}", cs.gray("Nothing here"));
            } else {
                let mut tp = TablePrinter::new(ios);
                for issue in issues {
                    let number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = issue.get("title").and_then(Value::as_str).unwrap_or("");
                    tp.add_row(vec![format!("  #{number}"), text::truncate(title, 60)]);
                }
                ios_println!(ios, "{}", tp.render());
            }
        } else {
            ios_println!(ios, "  {}", cs.gray("Nothing here"));
        }

        ios_println!(ios);

        // Recently opened issues
        let recent_issues = data
            .pointer("/recent/issues/nodes")
            .and_then(Value::as_array);

        ios_println!(ios, "{}", cs.bold("Recently opened issues"));

        if let Some(issues) = recent_issues {
            if issues.is_empty() {
                ios_println!(ios, "  {}", cs.gray("Nothing here"));
            } else {
                let mut tp = TablePrinter::new(ios);
                for issue in issues {
                    let number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
                    let title = issue.get("title").and_then(Value::as_str).unwrap_or("");
                    let author = issue
                        .pointer("/author/login")
                        .and_then(Value::as_str)
                        .unwrap_or("ghost");
                    tp.add_row(vec![
                        format!("  #{number}"),
                        text::truncate(title, 50),
                        cs.gray(author),
                    ]);
                }
                ios_println!(ios, "{}", tp.render());
            }
        } else {
            ios_println!(ios, "  {}", cs.gray("Nothing here"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    fn status_response() -> serde_json::Value {
        serde_json::json!({
            "data": {
                "assigned": {
                    "issues": {
                        "nodes": [
                            { "number": 1, "title": "Assigned Bug", "url": "https://github.com/owner/repo/issues/1", "updatedAt": "2024-01-15T10:00:00Z" }
                        ]
                    }
                },
                "mentioned": {
                    "issues": {
                        "nodes": [
                            { "number": 2, "title": "Mentioned Issue", "url": "https://github.com/owner/repo/issues/2", "updatedAt": "2024-01-15T10:00:00Z" }
                        ]
                    }
                },
                "recent": {
                    "issues": {
                        "nodes": [
                            { "number": 3, "title": "Recent Issue", "author": { "login": "alice" }, "url": "https://github.com/owner/repo/issues/3", "createdAt": "2024-01-15T10:00:00Z" }
                        ]
                    }
                }
            }
        })
    }

    fn viewer_response() -> serde_json::Value {
        serde_json::json!({
            "data": {
                "viewer": { "login": "testuser" }
            }
        })
    }

    #[tokio::test]
    async fn test_should_show_issue_status() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "UserCurrent", viewer_response()).await;
        mock_graphql(&h.server, "IssueStatus", status_response()).await;

        let args = StatusArgs {
            repo: "owner/repo".to_string(),
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("Assigned Bug"),
            "should contain assigned issue"
        );
        assert!(
            out.contains("Mentioned Issue"),
            "should contain mentioned issue"
        );
        assert!(out.contains("Recent Issue"), "should contain recent issue");
    }

    #[tokio::test]
    async fn test_should_output_json_when_requested() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "UserCurrent", viewer_response()).await;
        mock_graphql(&h.server, "IssueStatus", status_response()).await;

        let args = StatusArgs {
            repo: "owner/repo".to_string(),
            json: vec!["all".to_string()],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("assigned"), "should contain JSON data");
    }
}
