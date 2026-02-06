//! `ghc issue comment` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Add a comment to an issue.
#[derive(Debug, Args)]
pub struct CommentArgs {
    /// Issue number to comment on.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Comment body text.
    #[arg(short, long)]
    body: Option<String>,

    /// Open an editor for the comment body.
    #[arg(short, long)]
    editor: bool,

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

        let body = match (&self.body, self.editor) {
            (Some(b), _) => b.clone(),
            (None, true) => {
                let prompter = factory.prompter();
                prompter
                    .editor("Comment body", "", true)
                    .context("failed to read comment body from editor")?
            }
            (None, false) => {
                let prompter = factory.prompter();
                prompter
                    .input("Comment body", "")
                    .context("failed to read comment body")?
            }
        };

        if body.is_empty() {
            anyhow::bail!("comment body cannot be empty");
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

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
            editor: false,
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
}
