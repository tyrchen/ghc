//! `ghc repo unarchive` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Unarchive a GitHub repository.
///
/// With no argument, unarchives the current repository.
#[derive(Debug, Args)]
pub struct UnarchiveArgs {
    /// Repository to unarchive (OWNER/REPO).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// Skip the confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl UnarchiveArgs {
    /// Run the repo unarchive command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let repo = match &self.repo {
            Some(r) => Repo::from_full_name(r).context("invalid repository format")?,
            None => {
                anyhow::bail!("repository argument required (e.g. OWNER/REPO)")
            }
        };

        if !self.yes && !ios.can_prompt() {
            anyhow::bail!("--yes required when not running interactively");
        }

        let client = factory.api_client(repo.host())?;
        let full_name = repo.full_name();

        // Fetch repository to check if archived and get node ID
        let mut variables = HashMap::new();
        variables.insert("owner".into(), Value::String(repo.owner().to_string()));
        variables.insert("name".into(), Value::String(repo.name().to_string()));

        let data: Value = client
            .graphql(REPO_FIELDS_QUERY, &variables)
            .await
            .context("failed to fetch repository")?;

        let repo_data = data
            .get("repository")
            .ok_or_else(|| anyhow::anyhow!("repository not found: {full_name}"))?;

        let is_archived = repo_data
            .get("isArchived")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if !is_archived {
            ios_eprintln!(
                ios,
                "{} Repository {} is not archived",
                cs.warning_icon(),
                full_name
            );
            return Ok(());
        }

        // Confirm
        if !self.yes {
            let confirmed = factory
                .prompter()
                .confirm(&format!("Unarchive {full_name}?"), false)?;
            if !confirmed {
                return Ok(());
            }
        }

        let repo_id = repo_data
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("repository ID not found"))?;

        // Unarchive via GraphQL mutation
        let mut mutation_vars = HashMap::new();
        mutation_vars.insert(
            "input".into(),
            serde_json::json!({ "repositoryId": repo_id }),
        );

        let _: Value = client
            .graphql(UNARCHIVE_MUTATION, &mutation_vars)
            .await
            .context("failed to unarchive repository")?;

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Unarchived repository {}",
                cs.success_icon(),
                full_name
            );
        }

        Ok(())
    }
}

const REPO_FIELDS_QUERY: &str = r"
query RepositoryInfo($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    id
    name
    owner { login }
    isArchived
  }
}
";

const UNARCHIVE_MUTATION: &str = r"
mutation UnarchiveRepository($input: UnarchiveRepositoryInput!) {
  unarchiveRepository(input: $input) {
    repository {
      id
    }
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_unarchive_repository() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "RepositoryInfo",
            serde_json::json!({
                "data": {
                    "repository": {
                        "id": "R_123",
                        "name": "repo",
                        "owner": { "login": "owner" },
                        "isArchived": true,
                    }
                }
            }),
        )
        .await;

        mock_graphql(
            &h.server,
            "UnarchiveRepository",
            serde_json::json!({
                "data": {
                    "unarchiveRepository": {
                        "repository": { "id": "R_123" }
                    }
                }
            }),
        )
        .await;

        let args = UnarchiveArgs {
            repo: Some("owner/repo".into()),
            yes: true,
        };
        // Succeeds without error (TTY output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }

    #[tokio::test]
    async fn test_should_report_not_archived() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "RepositoryInfo",
            serde_json::json!({
                "data": {
                    "repository": {
                        "id": "R_123",
                        "name": "repo",
                        "owner": { "login": "owner" },
                        "isArchived": false,
                    }
                }
            }),
        )
        .await;

        let args = UnarchiveArgs {
            repo: Some("owner/repo".into()),
            yes: true,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("not archived"));
    }
}
