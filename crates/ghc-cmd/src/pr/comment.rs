//! `ghc pr comment` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Add a comment to a pull request.
#[derive(Debug, Args)]
pub struct CommentArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Comment body text.
    #[arg(short, long)]
    body: String,

    /// Open in web browser after commenting.
    #[arg(short, long)]
    web: bool,
}

impl CommentArgs {
    /// Run the pr comment command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/issues/{}/comments",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let body = serde_json::json!({ "body": self.body });
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to add comment")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Added comment to pull request #{}",
            cs.success_icon(),
            self.number,
        );

        if !html_url.is_empty() {
            ios_eprintln!(ios, "{html_url}");
            if self.web {
                factory.browser().open(html_url)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_add_comment_to_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/12/comments",
            201,
            serde_json::json!({
                "id": 100,
                "html_url": "https://github.com/owner/repo/pull/12#issuecomment-100"
            }),
        )
        .await;

        let args = CommentArgs {
            number: 12,
            repo: "owner/repo".into(),
            body: "Looks good!".into(),
            web: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Added comment"),
            "should confirm comment: {err}"
        );
        assert!(err.contains("#12"), "should contain PR number: {err}");
    }

    #[tokio::test]
    async fn test_should_comment_and_open_in_browser() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/13/comments",
            201,
            serde_json::json!({
                "id": 101,
                "html_url": "https://github.com/owner/repo/pull/13#issuecomment-101"
            }),
        )
        .await;

        let args = CommentArgs {
            number: 13,
            repo: "owner/repo".into(),
            body: "LGTM".into(),
            web: true,
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("issuecomment-101"));
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_comment() {
        let h = TestHarness::new().await;
        let args = CommentArgs {
            number: 1,
            repo: "bad".into(),
            body: "test".into(),
            web: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
