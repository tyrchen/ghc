//! `ghc issue delete` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Delete an issue.
///
/// Deleting an issue requires admin permissions on the repository. This
/// operation is irreversible and will prompt for confirmation unless
/// `--confirm` is passed.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Issue number to delete.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Skip confirmation prompt.
    #[arg(long)]
    confirm: bool,
}

impl DeleteArgs {
    /// Run the issue delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the user does not
    /// confirm, or the API request fails. Note that deleting issues requires
    /// admin permissions.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if !self.confirm {
            if !ios.can_prompt() {
                anyhow::bail!(
                    "deleting issue #{} requires confirmation; use --confirm to skip the prompt",
                    self.number,
                );
            }

            let prompter = factory.prompter();
            let confirmed = prompter
                .confirm(
                    &format!(
                        "You are about to permanently delete issue #{} in {}. This cannot be undone. Continue?",
                        self.number,
                        repo.full_name(),
                    ),
                    false,
                )
                .context("failed to read confirmation")?;

            if !confirmed {
                anyhow::bail!("delete cancelled");
            }
        }

        let client = factory.api_client(repo.host())?;

        // Issue deletion requires GraphQL - the REST API does not support it.
        // First, get the issue node ID.
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

        // Delete the issue via GraphQL mutation
        let mutation = r"
            mutation DeleteIssue($id: ID!) {
              deleteIssue(input: {issueId: $id}) {
                clientMutationId
              }
            }
        ";

        let mut mutation_vars = HashMap::new();
        mutation_vars.insert("id".to_string(), Value::String(node_id.to_string()));

        let _: Value = client
            .graphql(mutation, &mutation_vars)
            .await
            .context("failed to delete issue; you may need admin permissions")?;

        ios_eprintln!(
            ios,
            "{} Deleted issue #{} from {}",
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

    fn default_args(number: i32, repo: &str) -> DeleteArgs {
        DeleteArgs {
            number,
            repo: repo.to_string(),
            confirm: true,
        }
    }

    #[tokio::test]
    async fn test_should_delete_issue_with_confirm() {
        let h = TestHarness::new().await;
        // Mock the node ID query
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
        // Mock the delete mutation
        mock_graphql(
            &h.server,
            "DeleteIssue",
            serde_json::json!({
                "data": {
                    "deleteIssue": { "clientMutationId": null }
                }
            }),
        )
        .await;

        let args = default_args(42, "owner/repo");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Deleted issue #42"),
            "should show deleted message"
        );
    }

    #[tokio::test]
    async fn test_should_fail_without_confirm_in_non_tty() {
        let h = TestHarness::new().await;
        let mut args = default_args(42, "owner/repo");
        args.confirm = false;
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires confirmation")
        );
    }
}
