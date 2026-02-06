//! `ghc pr revert` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Revert a merged pull request by creating a new PR that undoes its changes.
#[derive(Debug, Args)]
pub struct RevertArgs {
    /// Pull request number to revert.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Title for the revert pull request. Defaults to "Revert #{number}".
    #[arg(short, long)]
    title: Option<String>,

    /// Body for the revert pull request.
    #[arg(short, long)]
    body: Option<String>,

    /// Do not create a pull request, only create the revert branch.
    #[arg(long)]
    branch_only: bool,
}

impl RevertArgs {
    /// Run the pr revert command.
    ///
    /// Reverts a merged pull request by fetching its merge commit, creating a
    /// revert commit on a new branch, and opening a new PR.
    ///
    /// # Errors
    ///
    /// Returns an error if the PR is not merged or the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Fetch the original PR to verify it is merged and get details
        let pr_path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let pr_data: Value = client
            .rest(reqwest::Method::GET, &pr_path, None::<&Value>)
            .await
            .context("failed to fetch pull request")?;

        let merged = pr_data
            .get("merged")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !merged {
            anyhow::bail!(
                "pull request #{} has not been merged and cannot be reverted",
                self.number,
            );
        }

        let merge_commit_sha = pr_data
            .get("merge_commit_sha")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "could not determine merge commit SHA for PR #{}",
                    self.number,
                )
            })?;
        let base_branch = pr_data
            .pointer("/base/ref")
            .and_then(Value::as_str)
            .unwrap_or("main");
        let original_title = pr_data.get("title").and_then(Value::as_str).unwrap_or("");

        let revert_branch = format!(
            "revert-{}-{}",
            self.number,
            &merge_commit_sha[..7.min(merge_commit_sha.len())]
        );

        // Create the revert branch and commit using git commands
        // First, fetch and ensure we are up to date
        let status = tokio::process::Command::new("git")
            .args(["fetch", "origin", base_branch])
            .status()
            .await
            .context("failed to fetch base branch")?;
        if !status.success() {
            anyhow::bail!("git fetch failed");
        }

        // Create a new branch from the base branch
        let status = tokio::process::Command::new("git")
            .args([
                "checkout",
                "-b",
                &revert_branch,
                &format!("origin/{base_branch}"),
            ])
            .status()
            .await
            .context("failed to create revert branch")?;
        if !status.success() {
            anyhow::bail!("failed to create revert branch");
        }

        // Revert the merge commit (parent 1 is the base branch side)
        let status = tokio::process::Command::new("git")
            .args(["revert", "--no-edit", "-m", "1", merge_commit_sha])
            .status()
            .await
            .context("failed to revert merge commit")?;
        if !status.success() {
            anyhow::bail!(
                "git revert failed for commit {merge_commit_sha}; you may need to resolve conflicts manually",
            );
        }

        // Push the revert branch
        let status = tokio::process::Command::new("git")
            .args(["push", "-u", "origin", &revert_branch])
            .status()
            .await
            .context("failed to push revert branch")?;
        if !status.success() {
            anyhow::bail!("git push failed");
        }

        ios_eprintln!(
            ios,
            "{} Created revert branch {revert_branch}",
            cs.success_icon(),
        );

        if self.branch_only {
            return Ok(());
        }

        // Create the revert PR
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| format!("Revert \"{}\" (PR #{})", original_title, self.number));
        let body = self.body.clone().unwrap_or_else(|| {
            format!(
                "Reverts {} (#{}).\n\nThis reverts merge commit {}.",
                repo.full_name(),
                self.number,
                merge_commit_sha,
            )
        });

        let pr_body = serde_json::json!({
            "title": title,
            "body": body,
            "head": revert_branch,
            "base": base_branch,
        });

        let create_path = format!("repos/{}/{}/pulls", repo.owner(), repo.name(),);
        let result: Value = client
            .rest(reqwest::Method::POST, &create_path, Some(&pr_body))
            .await
            .context("failed to create revert pull request")?;

        let new_number = result.get("number").and_then(Value::as_i64).unwrap_or(0);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Created revert pull request #{new_number}",
            cs.success_icon(),
        );
        ios_eprintln!(ios, "{html_url}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_fail_when_pr_not_merged() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/60",
            serde_json::json!({
                "number": 60,
                "state": "open",
                "merged": false,
                "title": "Open PR"
            }),
        )
        .await;

        let args = RevertArgs {
            number: 60,
            repo: "owner/repo".into(),
            title: None,
            body: None,
            branch_only: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("not been merged"),
            "should report PR not merged",
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_revert() {
        let h = TestHarness::new().await;
        let args = RevertArgs {
            number: 1,
            repo: "bad".into(),
            title: None,
            body: None,
            branch_only: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
