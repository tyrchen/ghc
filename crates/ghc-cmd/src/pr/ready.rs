//! `ghc pr ready` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Mark a draft pull request as ready for review.
#[derive(Debug, Args)]
pub struct ReadyArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,
}

impl ReadyArgs {
    /// Run the pr ready command.
    ///
    /// Uses the `markPullRequestReadyForReview` GraphQL mutation to transition
    /// a draft pull request to ready-for-review state.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the PR is not a draft.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Get the PR node ID
        let mut id_vars = HashMap::new();
        id_vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        id_vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        id_vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let pr_data: Value = client
            .graphql(
                r"query PrNodeId($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) {
                        pullRequest(number: $number) {
                            id
                            isDraft
                        }
                    }
                }",
                &id_vars,
            )
            .await
            .context("failed to fetch pull request")?;

        let pr_id = pr_data
            .pointer("/repository/pullRequest/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow::anyhow!("pull request #{} not found", self.number))?;

        let is_draft = pr_data
            .pointer("/repository/pullRequest/isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !is_draft {
            ios_eprintln!(
                ios,
                "{} Pull request #{} is already marked as ready",
                cs.warning_icon(),
                self.number,
            );
            return Ok(());
        }

        // Mark as ready for review
        let mut mutation_vars = HashMap::new();
        mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));

        let _: Value = client
            .graphql(
                r"mutation MarkReady($prId: ID!) {
                    markPullRequestReadyForReview(input: { pullRequestId: $prId }) {
                        pullRequest { isDraft }
                    }
                }",
                &mutation_vars,
            )
            .await
            .context("failed to mark pull request as ready for review")?;

        ios_eprintln!(
            ios,
            "{} Pull request #{} is now ready for review",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_mark_draft_pr_as_ready() {
        let h = TestHarness::new().await;

        // First mock: fetch PR node ID (draft)
        mock_graphql(
            &h.server,
            "PrNodeId",
            serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequest": {
                            "id": "PR_node123",
                            "isDraft": true
                        }
                    }
                }
            }),
        )
        .await;

        // Second mock: mark as ready mutation
        mock_graphql(
            &h.server,
            "MarkReady",
            serde_json::json!({
                "data": {
                    "markPullRequestReadyForReview": {
                        "pullRequest": { "isDraft": false }
                    }
                }
            }),
        )
        .await;

        let args = ReadyArgs {
            number: 15,
            repo: "owner/repo".into(),
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("ready for review"),
            "should confirm ready status: {err}",
        );
    }

    #[tokio::test]
    async fn test_should_warn_when_pr_already_ready() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "PrNodeId",
            serde_json::json!({
                "data": {
                    "repository": {
                        "pullRequest": {
                            "id": "PR_node456",
                            "isDraft": false
                        }
                    }
                }
            }),
        )
        .await;

        let args = ReadyArgs {
            number: 16,
            repo: "owner/repo".into(),
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("already marked as ready"),
            "should warn already ready: {err}",
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_ready() {
        let h = TestHarness::new().await;
        let args = ReadyArgs {
            number: 1,
            repo: "bad".into(),
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
