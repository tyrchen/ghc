//! `ghc issue reopen` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Reopen a closed issue.
#[derive(Debug, Args)]
pub struct ReopenArgs {
    /// Issue number to reopen.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Add a comment when reopening.
    #[arg(short, long)]
    comment: Option<String>,
}

impl ReopenArgs {
    /// Run the issue reopen command.
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

        let body = serde_json::json!({
            "state": "open",
        });

        let result: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to reopen issue")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Reopened issue #{} in {}",
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
    use crate::test_helpers::{TestHarness, mock_rest_patch};

    fn default_args(number: i32, repo: &str) -> ReopenArgs {
        ReopenArgs {
            number,
            repo: repo.to_string(),
            comment: None,
        }
    }

    #[tokio::test]
    async fn test_should_reopen_issue() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/issues/5",
            200,
            serde_json::json!({
                "html_url": "https://github.com/owner/repo/issues/5"
            }),
        )
        .await;

        let args = default_args(5, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Reopened issue #5"),
            "should show reopened message"
        );
        let out = h.stdout();
        assert!(
            out.contains("github.com/owner/repo/issues/5"),
            "should contain issue URL"
        );
    }
}
