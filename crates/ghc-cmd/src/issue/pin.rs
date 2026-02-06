//! `ghc issue pin` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Pin an issue to the repository.
///
/// Pinned issues appear at the top of the issues list. A repository can
/// have up to three pinned issues.
#[derive(Debug, Args)]
pub struct PinArgs {
    /// Issue number to pin.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,
}

impl PinArgs {
    /// Run the issue pin command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the issue is not
    /// found, the maximum number of pinned issues is reached, or the API
    /// request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Get the issue node ID via GraphQL
        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let query = r"
            query IssueNodeId($owner: String!, $name: String!, $number: Int!) {
              repository(owner: $owner, name: $name) {
                issue(number: $number) {
                  id
                }
              }
            }
        ";

        let data: Value = client
            .graphql(query, &variables)
            .await
            .context("failed to fetch issue")?;

        let node_id = data
            .pointer("/repository/issue/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("issue #{} not found in {}", self.number, repo.full_name())
            })?;

        // Pin the issue via GraphQL mutation
        let mutation = r"
            mutation PinIssue($id: ID!) {
              pinIssue(input: {issueId: $id}) {
                issue {
                  title
                }
              }
            }
        ";

        let mut mutation_vars = HashMap::new();
        mutation_vars.insert("id".to_string(), Value::String(node_id.to_string()));

        let _: Value = client
            .graphql(mutation, &mutation_vars)
            .await
            .context("failed to pin issue")?;

        ios_eprintln!(
            ios,
            "{} Pinned issue #{} in {}",
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
    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_pin_issue() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "IssueNodeId",
            serde_json::json!({
                "data": {
                    "repository": {
                        "issue": { "id": "I_abc123" }
                    }
                }
            }),
        )
        .await;
        mock_graphql(
            &h.server,
            "PinIssue",
            serde_json::json!({
                "data": {
                    "pinIssue": {
                        "issue": { "title": "Pinned Issue" }
                    }
                }
            }),
        )
        .await;

        let args = PinArgs {
            number: 5,
            repo: "owner/repo".to_string(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Pinned issue #5"),
            "should show pinned message"
        );
    }
}
