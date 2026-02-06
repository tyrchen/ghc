//! `ghc pr edit` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Edit a pull request.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// New title.
    #[arg(short, long)]
    title: Option<String>,

    /// New body.
    #[arg(short, long)]
    body: Option<String>,

    /// New base branch.
    #[arg(short = 'B', long)]
    base: Option<String>,

    /// Add labels (comma-separated).
    #[arg(long, value_delimiter = ',')]
    add_label: Vec<String>,

    /// Remove labels (comma-separated).
    #[arg(long, value_delimiter = ',')]
    remove_label: Vec<String>,

    /// Add assignees (comma-separated).
    #[arg(long, value_delimiter = ',')]
    add_assignee: Vec<String>,

    /// Remove assignees (comma-separated).
    #[arg(long, value_delimiter = ',')]
    remove_assignee: Vec<String>,

    /// Add reviewers (comma-separated).
    #[arg(long, value_delimiter = ',')]
    add_reviewer: Vec<String>,

    /// Remove reviewers (comma-separated).
    #[arg(long, value_delimiter = ',')]
    remove_reviewer: Vec<String>,

    /// Set milestone name or number.
    #[arg(short, long)]
    milestone: Option<String>,
}

impl EditArgs {
    /// Run the pr edit command.
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

        // Update PR fields if any are specified
        let mut pr_update = serde_json::Map::new();
        if let Some(ref title) = self.title {
            pr_update.insert("title".to_string(), Value::String(title.clone()));
        }
        if let Some(ref body) = self.body {
            pr_update.insert("body".to_string(), Value::String(body.clone()));
        }
        if let Some(ref base) = self.base {
            pr_update.insert("base".to_string(), Value::String(base.clone()));
        }
        if let Some(ref milestone) = self.milestone
            && let Ok(num) = milestone.parse::<u64>()
        {
            pr_update.insert(
                "milestone".to_string(),
                Value::Number(serde_json::Number::from(num)),
            );
        }

        if !pr_update.is_empty() {
            let path = format!(
                "repos/{}/{}/pulls/{}",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let body = Value::Object(pr_update);
            let _: Value = client
                .rest(reqwest::Method::PATCH, &path, Some(&body))
                .await
                .context("failed to update pull request")?;
        }

        // Add labels
        if !self.add_label.is_empty() {
            let labels_body = serde_json::json!({ "labels": self.add_label });
            let labels_path = format!(
                "repos/{}/{}/issues/{}/labels",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let _: Value = client
                .rest(reqwest::Method::POST, &labels_path, Some(&labels_body))
                .await
                .context("failed to add labels")?;
        }

        // Remove labels
        for label in &self.remove_label {
            let label_path = format!(
                "repos/{}/{}/issues/{}/labels/{}",
                repo.owner(),
                repo.name(),
                self.number,
                label,
            );
            let _ = client
                .rest_text(reqwest::Method::DELETE, &label_path, None)
                .await;
        }

        // Add assignees
        if !self.add_assignee.is_empty() {
            let assignees_body = serde_json::json!({ "assignees": self.add_assignee });
            let assignees_path = format!(
                "repos/{}/{}/issues/{}/assignees",
                repo.owner(),
                repo.name(),
                self.number,
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

        // Remove assignees
        if !self.remove_assignee.is_empty() {
            let assignees_body = serde_json::json!({ "assignees": self.remove_assignee });
            let assignees_path = format!(
                "repos/{}/{}/issues/{}/assignees",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::DELETE,
                    &assignees_path,
                    Some(&assignees_body),
                )
                .await
                .context("failed to remove assignees")?;
        }

        // Add reviewers
        if !self.add_reviewer.is_empty() {
            let reviewers_body = serde_json::json!({ "reviewers": self.add_reviewer });
            let reviewers_path = format!(
                "repos/{}/{}/pulls/{}/requested_reviewers",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::POST,
                    &reviewers_path,
                    Some(&reviewers_body),
                )
                .await
                .context("failed to add reviewers")?;
        }

        // Remove reviewers
        if !self.remove_reviewer.is_empty() {
            let reviewers_body = serde_json::json!({ "reviewers": self.remove_reviewer });
            let reviewers_path = format!(
                "repos/{}/{}/pulls/{}/requested_reviewers",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::DELETE,
                    &reviewers_path,
                    Some(&reviewers_body),
                )
                .await
                .context("failed to remove reviewers")?;
        }

        ios_eprintln!(
            ios,
            "{} Updated pull request #{}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_edit_pull_request_title() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/pulls/3",
            200,
            serde_json::json!({ "number": 3, "title": "Updated title" }),
        )
        .await;

        let args = EditArgs {
            number: 3,
            repo: "owner/repo".into(),
            title: Some("Updated title".into()),
            body: None,
            base: None,
            add_label: vec![],
            remove_label: vec![],
            add_assignee: vec![],
            remove_assignee: vec![],
            add_reviewer: vec![],
            remove_reviewer: vec![],
            milestone: None,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Updated pull request #3"),
            "should confirm edit: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_edit_pr_with_labels() {
        use crate::test_helpers::mock_rest_post;
        let h = TestHarness::new().await;

        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/3/labels",
            200,
            serde_json::json!([{ "name": "bug" }]),
        )
        .await;

        let args = EditArgs {
            number: 3,
            repo: "owner/repo".into(),
            title: None,
            body: None,
            base: None,
            add_label: vec!["bug".into()],
            remove_label: vec![],
            add_assignee: vec![],
            remove_assignee: vec![],
            add_reviewer: vec![],
            remove_reviewer: vec![],
            milestone: None,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Updated pull request #3"),
            "should confirm edit: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_edit() {
        let h = TestHarness::new().await;
        let args = EditArgs {
            number: 1,
            repo: "bad".into(),
            title: None,
            body: None,
            base: None,
            add_label: vec![],
            remove_label: vec![],
            add_assignee: vec![],
            remove_assignee: vec![],
            add_reviewer: vec![],
            remove_reviewer: vec![],
            milestone: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
