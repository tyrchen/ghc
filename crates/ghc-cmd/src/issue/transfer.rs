//! `ghc issue transfer` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Transfer an issue to another repository.
///
/// Both repositories must be owned by the same owner or organization.
/// The issue will be moved along with its comments, labels, and
/// assignees when possible.
#[derive(Debug, Args)]
pub struct TransferArgs {
    /// Issue number to transfer.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Source repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Destination repository in OWNER/REPO format.
    #[arg(value_name = "DESTINATION")]
    destination: String,
}

impl TransferArgs {
    /// Run the issue transfer command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the issue or
    /// destination repository is not found, or the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid source repository format")?;
        let dest_repo = ghc_core::repo::Repo::from_full_name(&self.destination)
            .context("invalid destination repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Get the issue node ID
        let mut issue_vars = HashMap::new();
        issue_vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        issue_vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        issue_vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let issue_query = r"
            query IssueNodeId($owner: String!, $name: String!, $number: Int!) {
              repository(owner: $owner, name: $name) {
                issue(number: $number) {
                  id
                }
              }
            }
        ";

        let issue_data: Value = client
            .graphql(issue_query, &issue_vars)
            .await
            .context("failed to fetch issue")?;

        let issue_id = issue_data
            .pointer("/repository/issue/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("issue #{} not found in {}", self.number, repo.full_name())
            })?;

        // Get the destination repository node ID
        let mut dest_vars = HashMap::new();
        dest_vars.insert(
            "owner".to_string(),
            Value::String(dest_repo.owner().to_string()),
        );
        dest_vars.insert(
            "name".to_string(),
            Value::String(dest_repo.name().to_string()),
        );

        let dest_query = r"
            query RepoNodeId($owner: String!, $name: String!) {
              repository(owner: $owner, name: $name) {
                id
              }
            }
        ";

        let dest_data: Value = client
            .graphql(dest_query, &dest_vars)
            .await
            .context("failed to fetch destination repository")?;

        let dest_id = dest_data
            .pointer("/repository/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("destination repository {} not found", dest_repo.full_name(),)
            })?;

        // Transfer the issue via GraphQL mutation
        let mutation = r"
            mutation TransferIssue($issueId: ID!, $repositoryId: ID!) {
              transferIssue(input: {issueId: $issueId, repositoryId: $repositoryId}) {
                issue {
                  number
                  url
                }
              }
            }
        ";

        let mut mutation_vars = HashMap::new();
        mutation_vars.insert("issueId".to_string(), Value::String(issue_id.to_string()));
        mutation_vars.insert(
            "repositoryId".to_string(),
            Value::String(dest_id.to_string()),
        );

        let result: Value = client
            .graphql(mutation, &mutation_vars)
            .await
            .context("failed to transfer issue")?;

        let new_url = result
            .pointer("/transferIssue/issue/url")
            .and_then(Value::as_str)
            .unwrap_or("");
        let new_number = result
            .pointer("/transferIssue/issue/number")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        ios_eprintln!(
            ios,
            "{} Transferred issue #{} from {} to {} as #{}",
            cs.success_icon(),
            self.number,
            cs.bold(&repo.full_name()),
            cs.bold(&dest_repo.full_name()),
            new_number,
        );

        if !new_url.is_empty() {
            ios_println!(ios, "{}", text::display_url(new_url));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_transfer_issue() {
        let h = TestHarness::new().await;
        // Mock fetching the issue node ID
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
        // Mock fetching the destination repo node ID
        mock_graphql(
            &h.server,
            "RepoNodeId",
            serde_json::json!({
                "data": {
                    "repository": { "id": "R_def456" }
                }
            }),
        )
        .await;
        // Mock the transfer mutation
        mock_graphql(
            &h.server,
            "TransferIssue",
            serde_json::json!({
                "data": {
                    "transferIssue": {
                        "issue": {
                            "number": 99,
                            "url": "https://github.com/owner/other-repo/issues/99"
                        }
                    }
                }
            }),
        )
        .await;

        let args = TransferArgs {
            number: 42,
            repo: "owner/repo".to_string(),
            destination: "owner/other-repo".to_string(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Transferred issue #42"),
            "should show transferred message"
        );
        let out = h.stdout();
        assert!(
            out.contains("github.com/owner/other-repo/issues/99"),
            "should contain new URL"
        );
    }
}
