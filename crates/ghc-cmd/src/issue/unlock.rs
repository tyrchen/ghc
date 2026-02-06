//! `ghc issue unlock` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Unlock an issue to allow conversation.
///
/// Unlocking an issue allows anyone to comment again.
#[derive(Debug, Args)]
pub struct UnlockArgs {
    /// Issue number to unlock.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,
}

impl UnlockArgs {
    /// Run the issue unlock command.
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

        // The unlock endpoint returns 204 No Content on success
        let _response = client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to unlock issue")?;

        ios_eprintln!(
            ios,
            "{} Unlocked issue #{} in {}",
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
    use crate::test_helpers::TestHarness;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    fn default_args(number: i32, repo: &str) -> UnlockArgs {
        UnlockArgs {
            number,
            repo: repo.to_string(),
        }
    }

    #[tokio::test]
    async fn test_should_unlock_issue() {
        let h = TestHarness::new().await;
        Mock::given(method("DELETE"))
            .and(path("/repos/owner/repo/issues/10/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let args = default_args(10, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Unlocked issue #10"),
            "should show unlocked message"
        );
    }
}
