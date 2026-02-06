//! `ghc issue list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// List issues in a repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Filter by issue state.
    #[arg(short, long, default_value = "open", value_parser = ["open", "closed", "all"])]
    state: String,

    /// Filter by assignee login. Use `@me` for the authenticated user.
    #[arg(short, long)]
    assignee: Option<String>,

    /// Filter by label names (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    label: Vec<String>,

    /// Filter by author login.
    #[arg(short = 'A', long)]
    author: Option<String>,

    /// Filter by milestone name.
    #[arg(short, long)]
    milestone: Option<String>,

    /// Search query to filter issues.
    #[arg(short = 'S', long)]
    search: Option<String>,

    /// Maximum number of issues to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Open the issue list in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the issue list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the API request
    /// fails, or the response cannot be parsed.
    #[allow(clippy::too_many_lines, clippy::cast_possible_wrap)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/issues",
                repo.host(),
                repo.owner(),
                repo.name(),
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let states = match self.state.as_str() {
            "open" => vec![Value::String("OPEN".to_string())],
            "closed" => vec![Value::String("CLOSED".to_string())],
            _ => vec![
                Value::String("OPEN".to_string()),
                Value::String("CLOSED".to_string()),
            ],
        };

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(self.limit.min(100))),
        );
        variables.insert("states".to_string(), Value::Array(states));

        if !self.label.is_empty() {
            let labels: Vec<Value> = self
                .label
                .iter()
                .map(|l| Value::String(l.clone()))
                .collect();
            variables.insert("labels".to_string(), Value::Array(labels));
        }

        if let Some(ref assignee) = self.assignee {
            variables.insert("assignee".to_string(), Value::String(assignee.clone()));
        }

        let data: Value = client
            .graphql(ghc_api::queries::issue::ISSUE_LIST_QUERY, &variables)
            .await
            .context("failed to list issues")?;

        let issues = data
            .pointer("/repository/issues/nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected API response format"))?;

        if issues.is_empty() {
            if ios.is_stdout_tty() {
                let cs = ios.color_scheme();
                ios_eprintln!(
                    ios,
                    "{} No issues match your search in {}",
                    cs.warning_icon(),
                    repo.full_name(),
                );
            }
            return Ok(());
        }

        // Apply client-side author filter if specified
        let filtered: Vec<&Value> = issues
            .iter()
            .filter(|issue| {
                if let Some(ref author) = self.author {
                    let issue_author = issue
                        .pointer("/author/login")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    issue_author.eq_ignore_ascii_case(author)
                } else {
                    true
                }
            })
            .collect();

        // JSON output mode
        if !self.json.is_empty() {
            let json_output =
                serde_json::to_string_pretty(&filtered).context("failed to serialize JSON")?;
            ios_println!(ios, "{json_output}");
            return Ok(());
        }

        // Table output
        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for issue in &filtered {
            let number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);
            let title = issue.get("title").and_then(Value::as_str).unwrap_or("");
            let state = issue.get("state").and_then(Value::as_str).unwrap_or("OPEN");

            let labels: Vec<&str> = issue
                .pointer("/labels/nodes")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.get("name").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();

            let created_at = issue.get("createdAt").and_then(Value::as_str).unwrap_or("");

            let state_display = if state == "OPEN" {
                cs.success("Open")
            } else {
                cs.magenta("Closed")
            };

            let label_display = if labels.is_empty() {
                String::new()
            } else {
                labels.join(", ")
            };

            tp.add_row(vec![
                format!("#{number}"),
                text::truncate(title, 60),
                label_display,
                state_display,
                created_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        if ios.is_stdout_tty() {
            ios_eprintln!(
                ios,
                "\nShowing {} of {} {}",
                filtered.len(),
                issues.len(),
                text::pluralize(issues.len() as i64, "issue", "issues"),
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{
        TestHarness, graphql_issue_list_response, issue_fixture, mock_graphql,
    };

    fn default_args(repo: &str) -> ListArgs {
        ListArgs {
            repo: repo.to_string(),
            state: "open".to_string(),
            assignee: None,
            label: vec![],
            author: None,
            milestone: None,
            search: None,
            limit: 30,
            web: false,
            json: vec![],
        }
    }

    #[tokio::test]
    async fn test_should_list_issues() {
        let h = TestHarness::new().await;
        let issues = vec![
            issue_fixture(1, "Bug fix", "OPEN"),
            issue_fixture(2, "Feature request", "OPEN"),
        ];
        mock_graphql(
            &h.server,
            "repository",
            graphql_issue_list_response(&issues),
        )
        .await;

        let args = default_args("owner/repo");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("#1"), "should contain issue number #1");
        assert!(out.contains("Bug fix"), "should contain first issue title");
        assert!(out.contains("#2"), "should contain issue number #2");
        assert!(
            out.contains("Feature request"),
            "should contain second issue title"
        );
    }

    #[tokio::test]
    async fn test_should_show_no_issues_message_when_empty() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "repository", graphql_issue_list_response(&[])).await;

        let args = default_args("owner/repo");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.is_empty(), "stdout should be empty for no issues");
    }

    #[tokio::test]
    async fn test_should_output_json_when_requested() {
        let h = TestHarness::new().await;
        let issues = vec![issue_fixture(1, "JSON test", "OPEN")];
        mock_graphql(
            &h.server,
            "repository",
            graphql_issue_list_response(&issues),
        )
        .await;

        let mut args = default_args("owner/repo");
        args.json = vec!["number".to_string(), "title".to_string()];
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("JSON test"),
            "should contain issue title in JSON output"
        );
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("owner/repo");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/issues"), "should open issues URL");
    }
}
