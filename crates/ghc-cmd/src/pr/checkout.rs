//! `ghc pr checkout` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Checkout a pull request branch locally.
#[derive(Debug, Args)]
pub struct CheckoutArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Local branch name to use (defaults to the PR head branch name).
    #[arg(short, long)]
    branch: Option<String>,

    /// Force checkout even if the branch already exists.
    #[arg(short, long)]
    force: bool,

    /// Detach HEAD (checkout without creating a branch).
    #[arg(long)]
    detach: bool,
}

impl CheckoutArgs {
    /// Run the pr checkout command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or git commands fail.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Fetch PR details to get head branch info
        let path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let pr_data: Value = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to fetch pull request")?;

        let head_ref = pr_data
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("could not determine head branch"))?;
        let head_repo_url = pr_data
            .pointer("/head/repo/clone_url")
            .and_then(Value::as_str)
            .unwrap_or("");
        let head_sha = pr_data
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .unwrap_or("");

        let local_branch = self.branch.as_deref().unwrap_or(head_ref);

        // Determine remote name (origin by default for same-repo PRs)
        let is_cross_repo = pr_data
            .pointer("/head/repo/full_name")
            .and_then(Value::as_str)
            .is_none_or(|name| {
                let base_full = pr_data
                    .pointer("/base/repo/full_name")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                name != base_full
            });

        if self.detach {
            // Fetch the specific commit and checkout in detached HEAD mode
            let fetch_ref = format!("pull/{}/head", self.number);
            let status = tokio::process::Command::new("git")
                .args(["fetch", "origin", &fetch_ref])
                .status()
                .await
                .context("failed to execute git fetch")?;
            if !status.success() {
                anyhow::bail!("git fetch failed");
            }

            let status = tokio::process::Command::new("git")
                .args(["checkout", "--detach", "FETCH_HEAD"])
                .status()
                .await
                .context("failed to execute git checkout")?;
            if !status.success() {
                anyhow::bail!("git checkout failed");
            }

            ios_eprintln!(
                ios,
                "{} Checked out PR #{} in detached HEAD at {:.7}",
                cs.success_icon(),
                self.number,
                head_sha,
            );
            return Ok(());
        }

        if is_cross_repo && !head_repo_url.is_empty() {
            // Cross-repo PR: fetch from the fork
            let fetch_ref = format!("{head_ref}:{local_branch}");
            let mut fetch_args = vec!["fetch", head_repo_url, &fetch_ref];
            if self.force {
                fetch_args.insert(1, "--force");
            }
            let status = tokio::process::Command::new("git")
                .args(&fetch_args)
                .status()
                .await
                .context("failed to execute git fetch from fork")?;
            if !status.success() {
                anyhow::bail!("git fetch from fork failed");
            }
        } else {
            // Same-repo PR: fetch from origin
            let fetch_ref = format!("pull/{}/head:{local_branch}", self.number);
            let mut fetch_args = vec!["fetch", "origin", &fetch_ref];
            if self.force {
                fetch_args.insert(1, "--force");
            }
            let status = tokio::process::Command::new("git")
                .args(&fetch_args)
                .status()
                .await
                .context("failed to execute git fetch")?;
            if !status.success() {
                anyhow::bail!("git fetch failed");
            }
        }

        // Checkout the local branch
        let status = tokio::process::Command::new("git")
            .args(["checkout", local_branch])
            .status()
            .await
            .context("failed to execute git checkout")?;
        if !status.success() {
            anyhow::bail!("git checkout failed");
        }

        ios_eprintln!(
            ios,
            "{} Checked out PR #{} on branch {local_branch}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_checkout() {
        let h = TestHarness::new().await;
        let args = CheckoutArgs {
            number: 1,
            repo: "bad".into(),
            branch: None,
            force: false,
            detach: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid repository")
        );
    }
}
