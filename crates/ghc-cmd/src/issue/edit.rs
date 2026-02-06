//! `ghc issue edit` command.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Edit one or more issues within the same repository.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Issue number(s) to edit.
    #[arg(value_name = "NUMBER", required = true, num_args = 1..)]
    numbers: Vec<i32>,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// New title for the issue.
    #[arg(short, long)]
    title: Option<String>,

    /// New body for the issue.
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Add assignees (comma-separated logins). Use "@me" to assign yourself.
    #[arg(long, value_delimiter = ',')]
    add_assignee: Vec<String>,

    /// Remove assignees (comma-separated logins). Use "@me" to unassign yourself.
    #[arg(long, value_delimiter = ',')]
    remove_assignee: Vec<String>,

    /// Add labels (comma-separated names).
    #[arg(long, value_delimiter = ',')]
    add_label: Vec<String>,

    /// Remove labels (comma-separated names).
    #[arg(long, value_delimiter = ',')]
    remove_label: Vec<String>,

    /// Add the issue to projects (comma-separated titles).
    #[arg(long, value_delimiter = ',')]
    add_project: Vec<String>,

    /// Remove the issue from projects (comma-separated titles).
    #[arg(long, value_delimiter = ',')]
    remove_project: Vec<String>,

    /// Set milestone name. Pass empty string to clear.
    #[arg(short, long, conflicts_with = "remove_milestone")]
    milestone: Option<String>,

    /// Remove the milestone association from the issue.
    #[arg(long, conflicts_with = "milestone")]
    remove_milestone: bool,
}

impl EditArgs {
    /// Run the issue edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, no fields are
    /// specified to edit, or the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(super::create::read_body_file(body_file).context("failed to read body file")?)
        } else {
            None
        };

        // Check that at least one field is being edited
        let has_edits = self.title.is_some()
            || self.body.is_some()
            || body_from_file.is_some()
            || !self.add_assignee.is_empty()
            || !self.remove_assignee.is_empty()
            || !self.add_label.is_empty()
            || !self.remove_label.is_empty()
            || !self.add_project.is_empty()
            || !self.remove_project.is_empty()
            || self.milestone.is_some()
            || self.remove_milestone;

        if !has_edits {
            anyhow::bail!(
                "no fields specified to edit; use --title, --body, --add-label, --add-assignee, or --milestone"
            );
        }

        for &number in &self.numbers {
            self.edit_single_issue(factory, &client, &repo, number, body_from_file.as_deref())
                .await?;

            ios_eprintln!(
                ios,
                "{} Edited issue #{number} in {}",
                cs.success_icon(),
                cs.bold(&repo.full_name()),
            );
        }

        Ok(())
    }

    /// Edit a single issue with the configured changes.
    #[allow(clippy::too_many_lines)]
    async fn edit_single_issue(
        &self,
        _factory: &crate::factory::Factory,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        number: i32,
        body_from_file: Option<&str>,
    ) -> Result<()> {
        let path = format!("repos/{}/{}/issues/{number}", repo.owner(), repo.name());

        // Build the update body with only specified fields
        let mut body = serde_json::Map::new();

        if let Some(ref title) = self.title {
            body.insert("title".to_string(), Value::String(title.clone()));
        }

        if let Some(ref issue_body) = self.body {
            body.insert("body".to_string(), Value::String(issue_body.clone()));
        } else if let Some(file_body) = body_from_file {
            body.insert("body".to_string(), Value::String(file_body.to_string()));
        }

        if self.remove_milestone {
            body.insert("milestone".to_string(), Value::Null);
        } else if let Some(ref milestone) = self.milestone {
            if milestone.is_empty() {
                body.insert("milestone".to_string(), Value::Null);
            } else {
                let milestone_number = resolve_milestone_number(client, repo, milestone).await?;
                body.insert(
                    "milestone".to_string(),
                    Value::Number(serde_json::Number::from(milestone_number)),
                );
            }
        }

        // Handle assignee changes
        if !self.add_assignee.is_empty() || !self.remove_assignee.is_empty() {
            // Resolve @me to the current user's login
            let mut add_logins: Vec<String> = self.add_assignee.clone();
            let mut remove_logins: Vec<String> = self.remove_assignee.clone();

            let has_at_me =
                add_logins.iter().any(|l| l == "@me") || remove_logins.iter().any(|l| l == "@me");
            if has_at_me {
                let me = resolve_current_user(client).await?;
                for login in &mut add_logins {
                    if login == "@me" {
                        login.clone_from(&me);
                    }
                }
                for login in &mut remove_logins {
                    if login == "@me" {
                        login.clone_from(&me);
                    }
                }
            }

            let current_assignees = fetch_current_assignees(client, repo, number).await?;
            let mut assignees: Vec<String> = current_assignees;

            for login in &add_logins {
                if !assignees.iter().any(|a| a.eq_ignore_ascii_case(login)) {
                    assignees.push(login.clone());
                }
            }

            assignees.retain(|a| !remove_logins.iter().any(|r| r.eq_ignore_ascii_case(a)));

            body.insert(
                "assignees".to_string(),
                Value::Array(assignees.into_iter().map(Value::String).collect()),
            );
        }

        // Handle label changes
        if !self.add_label.is_empty() || !self.remove_label.is_empty() {
            let current_labels = fetch_current_labels(client, repo, number).await?;
            let mut labels = current_labels;

            for label in &self.add_label {
                if !labels.iter().any(|l| l.eq_ignore_ascii_case(label)) {
                    labels.push(label.clone());
                }
            }

            labels.retain(|l| !self.remove_label.iter().any(|r| r.eq_ignore_ascii_case(l)));

            body.insert(
                "labels".to_string(),
                Value::Array(labels.into_iter().map(Value::String).collect()),
            );
        }

        if !body.is_empty() {
            let request_body = Value::Object(body);
            let _: Value = client
                .rest(reqwest::Method::PATCH, &path, Some(&request_body))
                .await
                .context("failed to edit issue")?;
        }

        Ok(())
    }
}

/// Resolve the current authenticated user's login via GET /user.
async fn resolve_current_user(client: &ghc_api::client::Client) -> Result<String> {
    let user: Value = client
        .rest(reqwest::Method::GET, "user", None::<&Value>)
        .await
        .context("failed to fetch current user")?;
    user.get("login")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("could not determine current user login"))
        .map(str::to_string)
}

/// Fetch the current assignees for an issue.
async fn fetch_current_assignees(
    client: &ghc_api::client::Client,
    repo: &ghc_core::repo::Repo,
    number: i32,
) -> Result<Vec<String>> {
    let path = format!("repos/{}/{}/issues/{}", repo.owner(), repo.name(), number,);

    let issue: Value = client
        .rest(reqwest::Method::GET, &path, None)
        .await
        .context("failed to fetch issue for assignees")?;

    let assignees = issue
        .get("assignees")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("login").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    Ok(assignees)
}

/// Fetch the current labels for an issue.
async fn fetch_current_labels(
    client: &ghc_api::client::Client,
    repo: &ghc_core::repo::Repo,
    number: i32,
) -> Result<Vec<String>> {
    let path = format!("repos/{}/{}/issues/{}", repo.owner(), repo.name(), number,);

    let issue: Value = client
        .rest(reqwest::Method::GET, &path, None)
        .await
        .context("failed to fetch issue for labels")?;

    let labels = issue
        .get("labels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    Ok(labels)
}

/// Resolve a milestone name to its number via the REST API.
async fn resolve_milestone_number(
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
    use crate::test_helpers::{TestHarness, mock_rest_patch};

    fn default_args(number: i32, repo: &str) -> EditArgs {
        EditArgs {
            numbers: vec![number],
            repo: repo.to_string(),
            title: None,
            body: None,
            body_file: None,
            add_assignee: vec![],
            remove_assignee: vec![],
            add_label: vec![],
            remove_label: vec![],
            add_project: vec![],
            remove_project: vec![],
            milestone: None,
            remove_milestone: false,
        }
    }

    #[tokio::test]
    async fn test_should_edit_issue_title() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/7",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/7"
            }),
        )
        .await;

        let mut args = default_args(7, "owner/repo");
        args.title = Some("Updated Title".to_string());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Edited issue #7"),
            "should show edited message"
        );
    }

    #[tokio::test]
    async fn test_should_fail_when_no_fields_specified() {
        let h = TestHarness::new().await;
        let args = default_args(7, "owner/repo");
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no fields specified")
        );
    }

    #[tokio::test]
    async fn test_should_edit_multiple_issues() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/7",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/7"
            }),
        )
        .await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/8",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/8"
            }),
        )
        .await;

        let mut args = default_args(7, "owner/repo");
        args.numbers = vec![7, 8];
        args.title = Some("Batch Title".to_string());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Edited issue #7"), "should show first: {err}");
        assert!(err.contains("Edited issue #8"), "should show second: {err}");
    }

    #[tokio::test]
    async fn test_should_remove_milestone() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/7",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/7"
            }),
        )
        .await;

        let mut args = default_args(7, "owner/repo");
        args.remove_milestone = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Edited issue #7"), "should show edited: {err}");
    }
}
