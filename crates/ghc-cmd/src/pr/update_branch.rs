//! `ghc pr update-branch` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Update method for the pull request branch.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum UpdateMethod {
    /// Merge the base branch into the PR branch.
    Merge,
    /// Rebase the PR branch onto the base branch.
    Rebase,
}

/// Update the branch of a pull request with the latest changes from the base branch.
#[derive(Debug, Args)]
pub struct UpdateBranchArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Update method.
    #[arg(short, long, value_enum, default_value = "merge")]
    method: UpdateMethod,
}

impl UpdateBranchArgs {
    /// Run the pr update-branch command.
    ///
    /// Uses `PUT /repos/{owner}/{repo}/pulls/{number}/update-branch` to merge
    /// or rebase the base branch into the PR branch.
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

        // First fetch the PR to get the head SHA (required by the API)
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

        let head_sha = pr_data
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("could not determine head SHA for PR #{}", self.number,)
            })?;

        let update_method = match self.method {
            UpdateMethod::Merge => "merge",
            UpdateMethod::Rebase => "rebase",
        };

        let path = format!(
            "repos/{}/{}/pulls/{}/update-branch",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let body = serde_json::json!({
            "expected_head_sha": head_sha,
            "update_method": update_method,
        });

        let _: Value = client
            .rest(reqwest::Method::PUT, &path, Some(&body))
            .await
            .context("failed to update pull request branch")?;

        ios_eprintln!(
            ios,
            "{} Updated branch for pull request #{} via {update_method}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_get};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    #[tokio::test]
    async fn test_should_update_pr_branch_via_merge() {
        let h = TestHarness::new().await;

        // Mock GET to fetch PR head SHA
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/25",
            serde_json::json!({
                "number": 25,
                "head": { "sha": "abc123def456" }
            }),
        )
        .await;

        // Mock PUT to update branch
        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/25/update-branch"))
            .respond_with(
                ResponseTemplate::new(202)
                    .set_body_json(serde_json::json!({ "message": "Updating" })),
            )
            .mount(&h.server)
            .await;

        let args = UpdateBranchArgs {
            number: 25,
            repo: "owner/repo".into(),
            method: UpdateMethod::Merge,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Updated branch for pull request #25"),
            "should confirm update: {err}",
        );
        assert!(err.contains("merge"), "should mention merge method: {err}");
    }

    #[tokio::test]
    async fn test_should_update_pr_branch_via_rebase() {
        let h = TestHarness::new().await;

        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/26",
            serde_json::json!({
                "number": 26,
                "head": { "sha": "def789" }
            }),
        )
        .await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/26/update-branch"))
            .respond_with(
                ResponseTemplate::new(202)
                    .set_body_json(serde_json::json!({ "message": "Updating" })),
            )
            .mount(&h.server)
            .await;

        let args = UpdateBranchArgs {
            number: 26,
            repo: "owner/repo".into(),
            method: UpdateMethod::Rebase,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("rebase"),
            "should mention rebase method: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_update_branch() {
        let h = TestHarness::new().await;
        let args = UpdateBranchArgs {
            number: 1,
            repo: "bad".into(),
            method: UpdateMethod::Merge,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
