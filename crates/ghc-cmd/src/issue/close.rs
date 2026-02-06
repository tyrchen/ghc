//! `ghc issue close` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Close an issue.
#[derive(Debug, Args)]
pub struct CloseArgs {
    /// Issue number to close.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Reason for closing the issue.
    #[arg(short, long, default_value = "completed", value_parser = ["completed", "not_planned"])]
    reason: String,

    /// Add a comment when closing.
    #[arg(short, long)]
    comment: Option<String>,
}

impl CloseArgs {
    /// Run the issue close command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the issue is not
    /// found, or the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Add comment first if provided
        if let Some(ref comment_body) = self.comment {
            let comment_path = format!(
                "repos/{}/{}/issues/{}/comments",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let comment_payload = serde_json::json!({ "body": comment_body });
            let _: Value = client
                .rest(reqwest::Method::POST, &comment_path, Some(&comment_payload))
                .await
                .context("failed to add comment")?;
        }

        let path = format!(
            "repos/{}/{}/issues/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );

        let state_reason = match self.reason.as_str() {
            "not_planned" => "not_planned",
            _ => "completed",
        };

        let body = serde_json::json!({
            "state": "closed",
            "state_reason": state_reason,
        });

        let result: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to close issue")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Closed issue #{} as {} in {}",
            cs.success_icon(),
            self.number,
            state_reason,
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{}", text::display_url(html_url));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_patch};

    fn default_args(number: i32, repo: &str) -> CloseArgs {
        CloseArgs {
            number,
            repo: repo.to_string(),
            reason: "completed".to_string(),
            comment: None,
        }
    }

    #[tokio::test]
    async fn test_should_close_issue() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/42",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/42"
            }),
        )
        .await;

        let args = default_args(42, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Closed issue #42"),
            "should show closed message"
        );
        let out = h.stdout();
        assert!(
            out.contains("github.com/owner/repo/issues/42"),
            "should contain issue URL"
        );
    }

    #[tokio::test]
    async fn test_should_close_issue_as_not_planned() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/10",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/10"
            }),
        )
        .await;

        let mut args = default_args(10, "owner/repo");
        args.reason = "not_planned".to_string();
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("not_planned"),
            "should show not_planned reason"
        );
    }
}
