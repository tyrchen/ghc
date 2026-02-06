//! `ghc issue comment` command.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Add a comment to an issue.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CommentArgs {
    /// Issue number to comment on.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Comment body text.
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Open an editor for the comment body.
    #[arg(short, long)]
    editor: bool,

    /// Edit the last comment of the current user.
    #[arg(long, conflicts_with_all = ["delete_last", "web"])]
    edit_last: bool,

    /// Delete the last comment of the current user.
    #[arg(long, conflicts_with_all = ["edit_last", "web"])]
    delete_last: bool,

    /// Skip the delete confirmation prompt when --delete-last is provided.
    #[arg(long)]
    yes: bool,

    /// Create a new comment if no comments are found. Used with --edit-last.
    #[arg(long, requires = "edit_last")]
    create_if_none: bool,

    /// Open the issue in the browser to add a comment.
    #[arg(short, long)]
    web: bool,
}

impl CommentArgs {
    /// Run the issue comment command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, no comment body
    /// is provided, or the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/issues/{}#issuecomment-new",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.number,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Handle --edit-last
        if self.edit_last {
            return self.handle_edit_last(factory, &client, &repo, ios).await;
        }

        // Handle --delete-last
        if self.delete_last {
            return self.handle_delete_last(factory, &client, &repo, ios).await;
        }

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(super::create::read_body_file(body_file).context("failed to read body file")?)
        } else {
            None
        };

        let body = match (&self.body, body_from_file, self.editor) {
            (Some(b), _, _) => b.clone(),
            (None, Some(b), _) => b,
            (None, None, true) => {
                let prompter = factory.prompter();
                prompter
                    .editor("Comment body", "", true)
                    .context("failed to read comment body from editor")?
            }
            (None, None, false) => {
                let prompter = factory.prompter();
                prompter
                    .input("Comment body", "")
                    .context("failed to read comment body")?
            }
        };

        if body.is_empty() {
            anyhow::bail!("comment body cannot be empty");
        }

        let path = format!(
            "repos/{}/{}/issues/{}/comments",
            repo.owner(),
            repo.name(),
            self.number,
        );

        let request_body = serde_json::json!({ "body": body });

        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&request_body))
            .await
            .context("failed to add comment")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Added comment to issue #{} in {}",
            cs.success_icon(),
            self.number,
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{}", text::display_url(html_url));

        Ok(())
    }

    /// Find the last comment by the authenticated user on the issue.
    async fn find_last_user_comment(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
    ) -> Result<Option<(i64, String, String)>> {
        // Get current user
        let user: Value = client
            .rest(reqwest::Method::GET, "user", None::<&Value>)
            .await
            .context("failed to fetch current user")?;
        let login = user.get("login").and_then(Value::as_str).unwrap_or("");

        // Get comments (most recent last)
        let path = format!(
            "repos/{}/{}/issues/{}/comments?per_page=100&direction=desc",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let comments: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to fetch comments")?;

        for comment in &comments {
            let author = comment
                .pointer("/user/login")
                .and_then(Value::as_str)
                .unwrap_or("");
            if author == login {
                let id = comment.get("id").and_then(Value::as_i64).unwrap_or(0);
                let body = comment
                    .get("body")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let html_url = comment
                    .get("html_url")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Ok(Some((id, body, html_url)));
            }
        }

        Ok(None)
    }

    /// Handle --edit-last: edit the user's last comment on the issue.
    async fn handle_edit_last(
        &self,
        factory: &crate::factory::Factory,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();

        let last_comment = self.find_last_user_comment(client, repo).await?;

        let Some((comment_id, existing_body, _html_url)) = last_comment else {
            if self.create_if_none {
                // Fall back to creating a new comment
                let prompter = factory.prompter();
                let body = prompter
                    .editor("Comment body", "", true)
                    .context("failed to read comment body")?;
                if body.is_empty() {
                    anyhow::bail!("comment body cannot be empty");
                }

                let path = format!(
                    "repos/{}/{}/issues/{}/comments",
                    repo.owner(),
                    repo.name(),
                    self.number,
                );
                let request_body = serde_json::json!({ "body": body });
                let result: Value = client
                    .rest(reqwest::Method::POST, &path, Some(&request_body))
                    .await
                    .context("failed to add comment")?;
                let url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

                ios_eprintln!(
                    ios,
                    "{} Added comment to issue #{} in {}",
                    cs.success_icon(),
                    self.number,
                    cs.bold(&repo.full_name()),
                );
                ios_println!(ios, "{}", text::display_url(url));
                return Ok(());
            }
            anyhow::bail!(
                "no comments found by the current user on issue #{}",
                self.number
            );
        };

        // Determine new body
        let new_body = if let Some(ref b) = self.body {
            b.clone()
        } else {
            let prompter = factory.prompter();
            prompter
                .editor("Comment body", &existing_body, true)
                .context("failed to read comment body from editor")?
        };

        if new_body.is_empty() {
            anyhow::bail!("comment body cannot be empty");
        }

        let path = format!(
            "repos/{}/{}/issues/comments/{}",
            repo.owner(),
            repo.name(),
            comment_id,
        );
        let request_body = serde_json::json!({ "body": new_body });
        let result: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&request_body))
            .await
            .context("failed to edit comment")?;

        let url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Edited comment on issue #{} in {}",
            cs.success_icon(),
            self.number,
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{}", text::display_url(url));

        Ok(())
    }

    /// Handle --delete-last: delete the user's last comment on the issue.
    async fn handle_delete_last(
        &self,
        factory: &crate::factory::Factory,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();

        let last_comment = self.find_last_user_comment(client, repo).await?;
        let (comment_id, _body, _html_url) = last_comment.ok_or_else(|| {
            anyhow::anyhow!(
                "no comments found by the current user on issue #{}",
                self.number,
            )
        })?;

        if !self.yes {
            if !ios.can_prompt() {
                anyhow::bail!("--yes required to delete comment in non-interactive mode");
            }
            let prompter = factory.prompter();
            let confirmed = prompter
                .confirm("Are you sure you want to delete your last comment?", false)
                .context("failed to read confirmation")?;
            if !confirmed {
                anyhow::bail!("delete cancelled");
            }
        }

        let path = format!(
            "repos/{}/{}/issues/comments/{}",
            repo.owner(),
            repo.name(),
            comment_id,
        );
        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete comment")?;

        ios_eprintln!(
            ios,
            "{} Deleted comment on issue #{} in {}",
            cs.success_icon(),
            self.number,
            cs.bold(&repo.full_name()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    fn default_args(number: i32, repo: &str) -> CommentArgs {
        CommentArgs {
            number,
            repo: repo.to_string(),
            body: Some("Test comment".to_string()),
            body_file: None,
            editor: false,
            edit_last: false,
            delete_last: false,
            yes: false,
            create_if_none: false,
            web: false,
        }
    }

    #[tokio::test]
    async fn test_should_add_comment() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/3/comments",
            201,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/3#issuecomment-1"
            }),
        )
        .await;

        let args = default_args(3, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Added comment to issue #3"),
            "should show added comment message"
        );
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args(3, "owner/repo");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("/issues/3#issuecomment-new"),
            "should open comment URL"
        );
    }

    #[tokio::test]
    async fn test_should_fail_with_empty_body() {
        let h = TestHarness::new().await;
        let mut args = default_args(3, "owner/repo");
        args.body = Some(String::new());
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_should_delete_last_comment() {
        use crate::test_helpers::{mock_rest_delete, mock_rest_get};

        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user",
            serde_json::json!({ "login": "testuser" }),
        )
        .await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/issues/3/comments",
            serde_json::json!([
                {
                    "id": 100,
                    "body": "My comment",
                    "html_url": "https://github.com/owner/repo/issues/3#issuecomment-100",
                    "user": { "login": "testuser" }
                }
            ]),
        )
        .await;
        mock_rest_delete(&h.server, "/repos/owner/repo/issues/comments/100", 204).await;

        let mut args = default_args(3, "owner/repo");
        args.body = None;
        args.delete_last = true;
        args.yes = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Deleted comment on issue #3"),
            "should show deleted message: {err}"
        );
    }
}
