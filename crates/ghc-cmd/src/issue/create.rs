//! `ghc issue create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Create a new issue.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Issue title.
    #[arg(short, long)]
    title: Option<String>,

    /// Issue body text.
    #[arg(short, long)]
    body: Option<String>,

    /// Assignee logins (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    assignee: Vec<String>,

    /// Label names (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    label: Vec<String>,

    /// Project names to add the issue to (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    project: Vec<String>,

    /// Milestone name.
    #[arg(short, long)]
    milestone: Option<String>,

    /// Open the new issue in the browser.
    #[arg(short, long)]
    web: bool,
}

impl CreateArgs {
    /// Run the issue create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, required fields
    /// are missing, or the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/issues/new",
                repo.host(),
                repo.owner(),
                repo.name(),
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let title = if let Some(t) = &self.title {
            t.clone()
        } else {
            let prompter = factory.prompter();
            prompter
                .input("Title", "")
                .context("failed to read title")?
        };

        if title.is_empty() {
            anyhow::bail!("title is required");
        }

        let body = if let Some(b) = &self.body {
            b.clone()
        } else {
            let prompter = factory.prompter();
            prompter
                .editor("Body", "", true)
                .context("failed to read body")?
        };

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!("repos/{}/{}/issues", repo.owner(), repo.name());
        let mut request_body = serde_json::json!({
            "title": title,
            "body": body,
        });

        if !self.assignee.is_empty() {
            request_body["assignees"] = Value::Array(
                self.assignee
                    .iter()
                    .map(|a| Value::String(a.clone()))
                    .collect(),
            );
        }

        if !self.label.is_empty() {
            request_body["labels"] = Value::Array(
                self.label
                    .iter()
                    .map(|l| Value::String(l.clone()))
                    .collect(),
            );
        }

        if let Some(ref milestone) = self.milestone {
            let milestone_number = resolve_milestone(&client, &repo, milestone).await?;
            request_body["milestone"] = Value::Number(serde_json::Number::from(milestone_number));
        }

        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&request_body))
            .await
            .context("failed to create issue")?;

        let number = result.get("number").and_then(Value::as_i64).unwrap_or(0);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Created issue #{} in {}",
            cs.success_icon(),
            number,
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{}", text::display_url(html_url));

        Ok(())
    }
}

/// Resolve a milestone name to its number via the REST API.
async fn resolve_milestone(
    client: &ghc_api::client::Client,
    repo: &ghc_core::repo::Repo,
    milestone_name: &str,
) -> Result<i64> {
    let path = format!(
        "repos/{}/{}/milestones?state=open&per_page=100",
        repo.owner(),
        repo.name(),
    );

    let milestones: Vec<Value> = client
        .rest(reqwest::Method::GET, &path, None)
        .await
        .context("failed to fetch milestones")?;

    for ms in &milestones {
        let title = ms.get("title").and_then(Value::as_str).unwrap_or("");
        if title.eq_ignore_ascii_case(milestone_name) {
            return ms
                .get("number")
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow::anyhow!("milestone missing number field"));
        }
    }

    anyhow::bail!(
        "milestone {milestone_name:?} not found; available milestones: {}",
        milestones
            .iter()
            .filter_map(|ms| ms.get("title").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    fn default_args(repo: &str) -> CreateArgs {
        CreateArgs {
            repo: repo.to_string(),
            title: Some("Test Issue".to_string()),
            body: Some("Test body".to_string()),
            assignee: vec![],
            label: vec![],
            project: vec![],
            milestone: None,
            web: false,
        }
    }

    #[tokio::test]
    async fn test_should_create_issue() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues",
            201,
            serde_json::json!({
                "number": 42,
                "html_url": "https://github.com/owner/repo/issues/42"
            }),
        )
        .await;

        let args = default_args("owner/repo");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("github.com/owner/repo/issues/42"),
            "should contain issue URL"
        );
        let err = h.stderr();
        assert!(
            err.contains("Created issue #42"),
            "should show created message"
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
        assert!(urls[0].contains("/issues/new"), "should open new issue URL");
    }

    #[tokio::test]
    async fn test_should_fail_with_empty_title() {
        let h = TestHarness::new().await;
        let mut args = default_args("owner/repo");
        args.title = Some(String::new());

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("title is required")
        );
    }
}
