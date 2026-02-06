//! `ghc pr close` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Close a pull request.
#[derive(Debug, Args)]
pub struct CloseArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Leave a comment when closing.
    #[arg(short, long)]
    comment: Option<String>,

    /// Delete the branch after closing.
    #[arg(short, long)]
    delete_branch: bool,
}

impl CloseArgs {
    /// Run the pr close command.
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

        // Close the pull request
        let path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let body = serde_json::json!({ "state": "closed" });
        let result: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to close pull request")?;

        // Delete branch if requested
        if self.delete_branch {
            let head_ref = result
                .pointer("/head/ref")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !head_ref.is_empty() {
                let ref_path = format!(
                    "repos/{}/{}/git/refs/heads/{}",
                    repo.owner(),
                    repo.name(),
                    head_ref,
                );
                // Deletion returns 204 No Content, so we use rest_text
                let _ = client
                    .rest_text(reqwest::Method::DELETE, &ref_path, None)
                    .await;
                ios_eprintln!(ios, "{} Deleted branch {head_ref}", cs.success_icon());
            }
        }

        ios_eprintln!(
            ios,
            "{} Closed pull request #{}",
            cs.success_icon(),
            self.number
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_close_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/pulls/5",
            200,
            serde_json::json!({ "state": "closed", "number": 5 }),
        )
        .await;

        let args = CloseArgs {
            number: 5,
            repo: "owner/repo".into(),
            comment: None,
            delete_branch: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Closed pull request #5"),
            "should confirm close: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_close_pr_with_comment() {
        use crate::test_helpers::mock_rest_post;
        let h = TestHarness::new().await;

        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues/5/comments",
            201,
            serde_json::json!({ "id": 1 }),
        )
        .await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/pulls/5",
            200,
            serde_json::json!({ "state": "closed", "number": 5 }),
        )
        .await;

        let args = CloseArgs {
            number: 5,
            repo: "owner/repo".into(),
            comment: Some("Closing this".into()),
            delete_branch: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Closed pull request #5"),
            "should confirm close: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_close() {
        let h = TestHarness::new().await;
        let args = CloseArgs {
            number: 1,
            repo: "bad".into(),
            comment: None,
            delete_branch: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
