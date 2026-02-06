//! `ghc issue lock` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Lock an issue to restrict conversation.
///
/// Once locked, only collaborators with at least triage access can add
/// new comments.
#[derive(Debug, Args)]
pub struct LockArgs {
    /// Issue number to lock.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Reason for locking the issue.
    #[arg(short, long, value_parser = ["off-topic", "too heated", "resolved", "spam"])]
    reason: Option<String>,
}

impl LockArgs {
    /// Run the issue lock command.
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

        // The lock endpoint returns 204 No Content on success, so use rest_text
        // to avoid JSON deserialization issues.
        let _response = client
            .rest_text(reqwest::Method::PUT, &path, body.as_ref())
            .await
            .context("failed to lock issue")?;

        let reason_display = self
            .reason
            .as_deref()
            .map(|r| format!(" as {r}"))
            .unwrap_or_default();

        ios_eprintln!(
            ios,
            "{} Locked issue #{}{} in {}",
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
    async fn test_should_lock_issue() {
        let h = TestHarness::new().await;
        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/issues/10/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let args = default_args(10, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Locked issue #10"),
            "should show locked message"
        );
    }

    #[tokio::test]
    async fn test_should_lock_issue_with_reason() {
        let h = TestHarness::new().await;
        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/issues/10/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let mut args = default_args(10, "owner/repo");
        args.reason = Some("spam".to_string());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("as spam"), "should show lock reason");
    }
}
