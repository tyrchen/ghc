//! `ghc project unlink` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Unlink a project from a repository.
#[derive(Debug, Args)]
pub struct UnlinkArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Repository to unlink (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: String,
}

impl UnlinkArgs {
    /// Run the project unlink command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be unlinked.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let repo = Repo::from_full_name(&self.repo).context("invalid repository format")?;
        let repo_id = super::link::resolve_repo_id(&client, repo.owner(), repo.name()).await?;

        let query = r"
            mutation UnlinkProject($projectId: ID!, $repositoryId: ID!) {
                unlinkProjectV2FromRepository(input: {
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
            .context("failed to unlink project from repository")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Unlinked project #{} from {}",
            cs.success_icon(),
            self.number,
            cs.bold(&self.repo),
        );

        Ok(())
    }
}
