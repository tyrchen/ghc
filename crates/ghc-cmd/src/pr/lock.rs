//! `ghc pr lock` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Lock a pull request conversation.
///
/// Once locked, only collaborators with at least triage access can add
/// new comments.
#[derive(Debug, Args)]
pub struct LockArgs {
    /// Pull request number to lock.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Reason for locking the conversation.
    #[arg(short, long, value_parser = ["off-topic", "too heated", "resolved", "spam"])]
    reason: Option<String>,
}

impl LockArgs {
    /// Run the pr lock command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the PR is not
    /// found, or the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // GitHub uses the issues API for PR lock/unlock
        let path = format!(
            "repos/{}/{}/issues/{}/lock",
            repo.owner(),
            repo.name(),
            self.number,
        );

        let body = self
            .reason
            .as_ref()
            .map(|reason| serde_json::json!({ "lock_reason": reason }));

        let _response = client
            .rest_text(reqwest::Method::PUT, &path, body.as_ref())
            .await
            .context("failed to lock pull request")?;

        let reason_display = self
            .reason
            .as_deref()
            .map(|r| format!(" as {r}"))
            .unwrap_or_default();

        ios_eprintln!(
            ios,
            "{} Locked pull request #{}{} in {}",
            cs.success_icon(),
            self.number,
            reason_display,
            cs.bold(&repo.full_name()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestHarness;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    fn default_args(number: i32, repo: &str) -> LockArgs {
        LockArgs {
            number,
            repo: repo.to_string(),
            reason: None,
        }
    }

    #[tokio::test]
    async fn test_should_lock_pr() {
        let h = TestHarness::new().await;
        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/issues/42/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let args = default_args(42, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Locked pull request #42"),
            "should show locked message"
        );
    }

    #[tokio::test]
    async fn test_should_lock_pr_with_reason() {
        let h = TestHarness::new().await;
        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/issues/42/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let mut args = default_args(42, "owner/repo");
        args.reason = Some("resolved".to_string());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("as resolved"), "should show lock reason");
    }
}
