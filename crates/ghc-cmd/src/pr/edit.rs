//! `ghc pr edit` command.

use std::path::PathBuf;

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
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// New base branch.
    #[arg(short = 'B', long)]
    base: Option<String>,

    /// Add labels (comma-separated).
    #[arg(long, value_delimiter = ',')]
    add_label: Vec<String>,

    /// Remove labels (comma-separated).
    #[arg(long, value_delimiter = ',')]
    remove_label: Vec<String>,

    /// Add assignees (comma-separated). Use "@me" to assign yourself.
    #[arg(long, value_delimiter = ',')]
    add_assignee: Vec<String>,

    /// Remove assignees (comma-separated). Use "@me" to unassign yourself.
    #[arg(long, value_delimiter = ',')]
    remove_assignee: Vec<String>,

    /// Add reviewers (comma-separated).
    #[arg(long, value_delimiter = ',')]
    add_reviewer: Vec<String>,

    /// Remove reviewers (comma-separated).
    #[arg(long, value_delimiter = ',')]
    remove_reviewer: Vec<String>,

    /// Add the pull request to projects (comma-separated titles).
    #[arg(long, value_delimiter = ',')]
    add_project: Vec<String>,

    /// Remove the pull request from projects (comma-separated titles).
    #[arg(long, value_delimiter = ',')]
    remove_project: Vec<String>,

    /// Set milestone name or number.
    #[arg(short, long, conflicts_with = "remove_milestone")]
    milestone: Option<String>,

    /// Remove the milestone association from the pull request.
    #[arg(long, conflicts_with = "milestone")]
    remove_milestone: bool,
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

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(
                crate::issue::create::read_body_file(body_file)
                    .context("failed to read body file")?,
            )
        } else {
            None
        };

        // Update PR fields if any are specified
        let mut pr_update = serde_json::Map::new();
        if let Some(ref title) = self.title {
            pr_update.insert("title".to_string(), Value::String(title.clone()));
        }
        if let Some(ref body) = self.body {
            pr_update.insert("body".to_string(), Value::String(body.clone()));
        } else if let Some(ref body) = body_from_file {
            pr_update.insert("body".to_string(), Value::String(body.clone()));
        }
        if let Some(ref base) = self.base {
            pr_update.insert("base".to_string(), Value::String(base.clone()));
        }
        if self.remove_milestone {
            pr_update.insert("milestone".to_string(), Value::Null);
        } else if let Some(ref milestone) = self.milestone
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

    fn default_edit_args(number: i64, repo: &str) -> EditArgs {
        EditArgs {
            number,
            repo: repo.into(),
            title: None,
            body: None,
            body_file: None,
            base: None,
            add_label: vec![],
            remove_label: vec![],
            add_assignee: vec![],
            remove_assignee: vec![],
            add_reviewer: vec![],
            remove_reviewer: vec![],
            add_project: vec![],
            remove_project: vec![],
            milestone: None,
            remove_milestone: false,
        }
    }

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

        let mut args = default_edit_args(3, "owner/repo");
        args.title = Some("Updated title".into());
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

        let mut args = default_edit_args(3, "owner/repo");
        args.add_label = vec!["bug".into()];
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
        let args = default_edit_args(1, "bad");
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
