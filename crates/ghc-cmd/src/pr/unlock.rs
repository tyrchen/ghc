//! `ghc pr unlock` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Unlock a pull request conversation.
///
/// Unlocking a pull request allows anyone to comment again.
#[derive(Debug, Args)]
pub struct UnlockArgs {
    /// Pull request number to unlock.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,
}

impl UnlockArgs {
    /// Run the pr unlock command.
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

        let _response = client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to unlock pull request")?;

        ios_eprintln!(
            ios,
            "{} Unlocked pull request #{} in {}",
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
    async fn test_should_unlock_pr() {
        let h = TestHarness::new().await;
        Mock::given(method("DELETE"))
            .and(path("/repos/owner/repo/issues/42/lock"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&h.server)
            .await;

        let args = default_args(42, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Unlocked pull request #42"),
            "should show unlocked message"
        );
    }
}
