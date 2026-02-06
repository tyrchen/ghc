//! `ghc project delete` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Delete a project.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,
}

impl DeleteArgs {
    /// Run the project delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation DeleteProject($projectId: ID!) {
                deleteProjectV2(input: { projectId: $projectId }) {
                    projectV2 { id }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to delete project")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted project #{}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}
