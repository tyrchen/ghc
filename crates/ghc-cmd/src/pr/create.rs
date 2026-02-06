//! `ghc pr create` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Create a pull request.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Title of the pull request.
    #[arg(short, long)]
    title: String,

    /// Body of the pull request.
    #[arg(short, long, default_value = "")]
    body: String,

    /// Base branch (target branch for merge).
    #[arg(short = 'B', long)]
    base: Option<String>,

    /// Head branch (source branch with changes).
    #[arg(short = 'H', long)]
    head: String,

    /// Create as a draft pull request.
    #[arg(short, long)]
    draft: bool,

    /// Labels to add.
    #[arg(short, long)]
    label: Vec<String>,

    /// Assignees to add (by login).
    #[arg(short, long)]
    assignee: Vec<String>,

    /// Reviewers to request (by login).
    #[arg(short, long)]
    reviewer: Vec<String>,

    /// Milestone name or number.
    #[arg(short, long)]
    milestone: Option<String>,

    /// Open in web browser after creating.
    #[arg(short, long)]
    web: bool,
}

impl CreateArgs {
    /// Run the pr create command.
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

        // Determine base branch
        let base = if let Some(b) = &self.base {
            b.clone()
        } else {
            // Fetch default branch from the repo
            let mut vars = HashMap::new();
            vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
            vars.insert("name".to_string(), Value::String(repo.name().to_string()));
            let data: Value = client
                .graphql(
                    r"query DefaultBranch($owner: String!, $name: String!) {
                            repository(owner: $owner, name: $name) {
                                defaultBranchRef { name }
                            }
                        }",
                    &vars,
                )
                .await
                .context("failed to determine default branch")?;
            data.pointer("/repository/defaultBranchRef/name")
                .and_then(Value::as_str)
                .unwrap_or("main")
                .to_string()
        };

        let mut body = serde_json::json!({
            "title": self.title,
            "body": self.body,
            "head": self.head,
            "base": base,
            "draft": self.draft,
        });

        if let Some(ref milestone) = self.milestone {
            // Try to parse as number first, otherwise treat as name
            if let Ok(num) = milestone.parse::<u64>() {
                body["milestone"] = Value::Number(serde_json::Number::from(num));
            }
        }

        let path = format!("repos/{}/{}/pulls", repo.owner(), repo.name());
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to create pull request")?;

        let number = result.get("number").and_then(Value::as_i64).unwrap_or(0);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        // Add labels if specified
        if !self.label.is_empty() {
            let labels_body = serde_json::json!({ "labels": self.label });
            let labels_path = format!(
                "repos/{}/{}/issues/{}/labels",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(reqwest::Method::POST, &labels_path, Some(&labels_body))
                .await
                .context("failed to add labels")?;
        }

        // Add assignees if specified
        if !self.assignee.is_empty() {
            let assignees_body = serde_json::json!({ "assignees": self.assignee });
            let assignees_path = format!(
                "repos/{}/{}/issues/{}/assignees",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::POST,
                    &assignees_path,
                    Some(&assignees_body),
                )
                .await
                .context("failed to add assignees")?;
        }

        // Request reviewers if specified
        if !self.reviewer.is_empty() {
            let reviewers_body = serde_json::json!({ "reviewers": self.reviewer });
            let reviewers_path = format!(
                "repos/{}/{}/pulls/{}/requested_reviewers",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::POST,
                    &reviewers_path,
                    Some(&reviewers_body),
                )
                .await
                .context("failed to request reviewers")?;
        }

        ios_eprintln!(
            ios,
            "{} Created pull request #{} in {}",
            cs.success_icon(),
            cs.bold(&number.to_string()),
            repo.full_name(),
        );
        ios_eprintln!(ios, "{html_url}");

        if self.web && !html_url.is_empty() {
            factory.browser().open(html_url)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls",
            201,
            serde_json::json!({
                "number": 10,
                "html_url": "https://github.com/owner/repo/pull/10"
            }),
        )
        .await;

        let args = CreateArgs {
            repo: "owner/repo".into(),
            title: "New feature".into(),
            body: "Description".into(),
            base: Some("main".into()),
            head: "feature-branch".into(),
            draft: false,
            label: vec![],
            assignee: vec![],
            reviewer: vec![],
            milestone: None,
            web: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Created pull request #10"),
            "should confirm creation: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_create_pr_and_open_in_browser() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls",
            201,
            serde_json::json!({
                "number": 11,
                "html_url": "https://github.com/owner/repo/pull/11"
            }),
        )
        .await;

        let args = CreateArgs {
            repo: "owner/repo".into(),
            title: "Browser test".into(),
            body: String::new(),
            base: Some("main".into()),
            head: "feat".into(),
            draft: false,
            label: vec![],
            assignee: vec![],
            reviewer: vec![],
            milestone: None,
            web: true,
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/pull/11"));
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_create() {
        let h = TestHarness::new().await;
        let args = CreateArgs {
            repo: "bad".into(),
            title: "T".into(),
            body: String::new(),
            base: Some("main".into()),
            head: "feat".into(),
            draft: false,
            label: vec![],
            assignee: vec![],
            reviewer: vec![],
            milestone: None,
            web: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
