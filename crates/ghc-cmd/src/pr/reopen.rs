//! `ghc pr reopen` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Reopen a closed pull request.
#[derive(Debug, Args)]
pub struct ReopenArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Leave a comment when reopening.
    #[arg(short, long)]
    comment: Option<String>,
}

impl ReopenArgs {
    /// Run the pr reopen command.
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

        // Add comment if specified
        if let Some(ref comment_body) = self.comment {
            let comment_path = format!(
                "repos/{}/{}/issues/{}/comments",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let comment_data = serde_json::json!({ "body": comment_body });
            let _: Value = client
                .rest(reqwest::Method::POST, &comment_path, Some(&comment_data))
                .await
                .context("failed to add comment")?;
        }

        // Reopen the pull request
        let path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let body = serde_json::json!({ "state": "open" });
        let _: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to reopen pull request")?;

        ios_eprintln!(
            ios,
            "{} Reopened pull request #{}",
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
    async fn test_should_reopen_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/pulls/7",
            200,
            serde_json::json!({ "state": "open", "number": 7 }),
        )
        .await;

        let args = ReopenArgs {
            number: 7,
            repo: "owner/repo".into(),
            comment: None,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Reopened pull request #7"),
            "should confirm reopen: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_reopen_pr_with_comment() {
        use crate::test_helpers::mock_rest_post;
        let h = TestHarness::new().await;

        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/7/comments",
            201,
            serde_json::json!({ "id": 1 }),
        )
        .await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/pulls/7",
            200,
            serde_json::json!({ "state": "open", "number": 7 }),
        )
        .await;

        let args = ReopenArgs {
            number: 7,
            repo: "owner/repo".into(),
            comment: Some("Reopening".into()),
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Reopened pull request #7"),
            "should confirm reopen: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_reopen() {
        let h = TestHarness::new().await;
        let args = ReopenArgs {
            number: 1,
            repo: "bad".into(),
            comment: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
