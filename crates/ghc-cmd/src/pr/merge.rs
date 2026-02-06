//! `ghc pr merge` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Merge method for a pull request.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum MergeMethod {
    /// Standard merge commit.
    Merge,
    /// Squash and merge into a single commit.
    Squash,
    /// Rebase and merge without a merge commit.
    Rebase,
}

/// Merge a pull request.
#[derive(Debug, Args)]
pub struct MergeArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Merge method to use.
    #[arg(short, long, value_enum, default_value = "merge")]
    method: MergeMethod,

    /// Commit title for the merge commit.
    #[arg(short, long)]
    subject: Option<String>,

    /// Commit message body for the merge commit.
    #[arg(short, long)]
    body: Option<String>,

    /// Delete the branch after merging.
    #[arg(short, long)]
    delete_branch: bool,

    /// Enable auto-merge when requirements are met.
    #[arg(long)]
    auto: bool,

    /// Disable auto-merge for this pull request.
    #[arg(long)]
    disable_auto: bool,
}

impl MergeArgs {
    /// Run the pr merge command.
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

        // Handle auto-merge enable/disable via GraphQL
        if self.auto || self.disable_auto {
            return self.handle_auto_merge(&client, &repo, ios).await;
        }

        let merge_method = match self.method {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
        };

        let mut body = serde_json::json!({
            "merge_method": merge_method,
        });

        if let Some(ref subject) = self.subject {
            body["commit_title"] = Value::String(subject.clone());
        }
        if let Some(ref msg) = self.body {
            body["commit_message"] = Value::String(msg.clone());
        }

        let path = format!(
            "repos/{}/{}/pulls/{}/merge",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let result: Value = client
            .rest(reqwest::Method::PUT, &path, Some(&body))
            .await
            .context("failed to merge pull request")?;

        let merged = result
            .get("merged")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if merged {
            ios_eprintln!(
                ios,
                "{} Merged pull request #{} via {merge_method}",
                cs.success_icon(),
                self.number,
            );
        } else {
            let message = result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown reason");
            anyhow::bail!(
                "pull request #{} could not be merged: {message}",
                self.number,
            );
        }

        // Delete branch if requested
        if self.delete_branch {
            // Fetch the PR to get the head ref
            let pr_path = format!(
                "repos/{}/{}/pulls/{}",
                repo.owner(),
                repo.name(),
                self.number,
            );
            let pr_data: Value = client
                .rest(reqwest::Method::GET, &pr_path, None::<&Value>)
                .await
                .context("failed to fetch pull request details")?;

            let head_ref = pr_data
                .pointer("/head/ref")
                .and_then(Value::as_str)
                .unwrap_or("");

            if !head_ref.is_empty() {
                let ref_path = format!(
                    "repos/{}/{}/git/refs/heads/{}",
                    repo.owner(),
                    repo.name(),
                    head_ref,
                );
                let _ = client
                    .rest_text(reqwest::Method::DELETE, &ref_path, None)
                    .await;
                ios_eprintln!(ios, "{} Deleted branch {head_ref}", cs.success_icon());
            }
        }

        Ok(())
    }

    /// Handle auto-merge enable/disable via GraphQL mutations.
    async fn handle_auto_merge(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();
        // First get the PR node ID
        let mut vars = std::collections::HashMap::new();
        vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let pr_data: Value = client
            .graphql(
                r"query PrNodeId($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) {
                        pullRequest(number: $number) { id }
                    }
                }",
                &vars,
            )
            .await
            .context("failed to fetch pull request ID")?;

        let pr_id = pr_data
            .pointer("/repository/pullRequest/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow::anyhow!("pull request #{} not found", self.number))?;

        if self.disable_auto {
            let mut mutation_vars = std::collections::HashMap::new();
            mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));

            let _: Value = client
                .graphql(
                    r"mutation DisableAutoMerge($prId: ID!) {
                        disablePullRequestAutoMerge(input: { pullRequestId: $prId }) {
                            pullRequest { number }
                        }
                    }",
                    &mutation_vars,
                )
                .await
                .context("failed to disable auto-merge")?;

            ios_eprintln!(
                ios,
                "{} Disabled auto-merge for pull request #{}",
                cs.success_icon(),
                self.number,
            );
        } else {
            let merge_method = match self.method {
                MergeMethod::Merge => "MERGE",
                MergeMethod::Squash => "SQUASH",
                MergeMethod::Rebase => "REBASE",
            };

            let mut mutation_vars = std::collections::HashMap::new();
            mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));
            mutation_vars.insert(
                "mergeMethod".to_string(),
                Value::String(merge_method.to_string()),
            );

            let _: Value = client
                .graphql(
                    r"mutation EnableAutoMerge($prId: ID!, $mergeMethod: PullRequestMergeMethod!) {
                        enablePullRequestAutoMerge(input: { pullRequestId: $prId, mergeMethod: $mergeMethod }) {
                            pullRequest { number }
                        }
                    }",
                    &mutation_vars,
                )
                .await
                .context("failed to enable auto-merge")?;

            ios_eprintln!(
                ios,
                "{} Enabled auto-merge for pull request #{} ({merge_method})",
                cs.success_icon(),
                self.number,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestHarness;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    #[tokio::test]
    async fn test_should_merge_pull_request() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/8/merge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "merged": true, "sha": "abc123" })),
            )
            .mount(&h.server)
            .await;

        let args = MergeArgs {
            number: 8,
            repo: "owner/repo".into(),
            method: MergeMethod::Merge,
            subject: None,
            body: None,
            delete_branch: false,
            auto: false,
            disable_auto: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Merged pull request #8"),
            "should confirm merge: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_merge_with_squash_method() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/9/merge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "merged": true, "sha": "def456" })),
            )
            .mount(&h.server)
            .await;

        let args = MergeArgs {
            number: 9,
            repo: "owner/repo".into(),
            method: MergeMethod::Squash,
            subject: Some("Squash commit".into()),
            body: None,
            delete_branch: false,
            auto: false,
            disable_auto: false,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("via squash"),
            "should mention squash method: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_fail_when_merge_not_possible() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/10/merge"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "merged": false, "message": "checks pending" }),
                ),
            )
            .mount(&h.server)
            .await;

        let args = MergeArgs {
            number: 10,
            repo: "owner/repo".into(),
            method: MergeMethod::Merge,
            subject: None,
            body: None,
            delete_branch: false,
            auto: false,
            disable_auto: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("could not be merged")
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_merge() {
        let h = TestHarness::new().await;
        let args = MergeArgs {
            number: 1,
            repo: "bad".into(),
            method: MergeMethod::Merge,
            subject: None,
            body: None,
            delete_branch: false,
            auto: false,
            disable_auto: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
