//! `ghc project link` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Link a project to a repository.
#[derive(Debug, Args)]
pub struct LinkArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Repository to link (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: String,
}

impl LinkArgs {
    /// Run the project link command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be linked.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let repo = Repo::from_full_name(&self.repo).context("invalid repository format")?;
        let repo_id = resolve_repo_id(&client, repo.owner(), repo.name()).await?;

        let query = r"
            mutation LinkProject($projectId: ID!, $repositoryId: ID!) {
                linkProjectV2ToRepository(input: {
                    projectId: $projectId,
                    repositoryId: $repositoryId
                }) {
                    repository { id }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("repositoryId".to_string(), Value::String(repo_id));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to link project to repository")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Linked project #{} to {}",
            cs.success_icon(),
            self.number,
            cs.bold(&self.repo),
        );

        Ok(())
    }
}

/// Resolve a repository's node ID from owner and name.
pub(super) async fn resolve_repo_id(
    client: &ghc_api::client::Client,
    owner: &str,
    name: &str,
) -> Result<String> {
    let query = r"
        query FindRepo($owner: String!, $name: String!) {
            repository(owner: $owner, name: $name) { id }
        }
    ";

    let mut vars = HashMap::new();
    vars.insert("owner".to_string(), Value::String(owner.to_string()));
    vars.insert("name".to_string(), Value::String(name.to_string()));

    let data: Value = client
        .graphql(query, &vars)
        .await
        .context("failed to resolve repository")?;

    data.pointer("/repository/id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("repository not found: {owner}/{name}"))
}
