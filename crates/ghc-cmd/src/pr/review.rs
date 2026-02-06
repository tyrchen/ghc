//! `ghc pr review` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Review event type.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ReviewEvent {
    /// Approve the pull request.
    Approve,
    /// Request changes on the pull request.
    RequestChanges,
    /// Leave a comment without explicit approval or rejection.
    Comment,
}

/// Add a review to a pull request.
#[derive(Debug, Args)]
pub struct ReviewArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Review action to take.
    #[arg(short, long, value_enum)]
    event: ReviewEvent,

    /// Review body/comment.
    #[arg(short, long, default_value = "")]
    body: String,
}

impl ReviewArgs {
    /// Run the pr review command.
    ///
    /// Submits a pull request review via `POST /repos/{owner}/{repo}/pulls/{number}/reviews`
    /// with the specified event type (`APPROVE`, `REQUEST_CHANGES`, or `COMMENT`).
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

        let event = match self.event {
            ReviewEvent::Approve => "APPROVE",
            ReviewEvent::RequestChanges => "REQUEST_CHANGES",
            ReviewEvent::Comment => "COMMENT",
        };

        let path = format!(
            "repos/{}/{}/pulls/{}/reviews",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let body = serde_json::json!({
            "event": event,
            "body": self.body,
        });

        let _: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to submit review")?;

        let action_display = match self.event {
            ReviewEvent::Approve => cs.success("Approved"),
            ReviewEvent::RequestChanges => cs.warning("Requested changes on"),
            ReviewEvent::Comment => "Reviewed".to_string(),
        };

        ios_eprintln!(
            ios,
            "{} {action_display} pull request #{}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_approve_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls/20/reviews",
            200,
            serde_json::json!({ "id": 1, "state": "APPROVED" }),
        )
        .await;

        let args = ReviewArgs {
            number: 20,
            repo: "owner/repo".into(),
            event: ReviewEvent::Approve,
            body: String::new(),
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(err.contains("Approved"), "should confirm approval: {err}");
        assert!(err.contains("#20"), "should contain PR number: {err}");
    }

    #[tokio::test]
    async fn test_should_request_changes_on_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls/21/reviews",
            200,
            serde_json::json!({ "id": 2, "state": "CHANGES_REQUESTED" }),
        )
        .await;

        let args = ReviewArgs {
            number: 21,
            repo: "owner/repo".into(),
            event: ReviewEvent::RequestChanges,
            body: "Please fix the tests".into(),
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Requested changes"),
            "should confirm changes requested: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_review() {
        let h = TestHarness::new().await;
        let args = ReviewArgs {
            number: 1,
            repo: "bad".into(),
            event: ReviewEvent::Approve,
            body: String::new(),
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
