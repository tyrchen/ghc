//! `ghc project create` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::{ios_eprintln, ios_println};
use serde_json::Value;

/// Create a project.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Project title.
    #[arg(value_name = "TITLE")]
    title: String,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,
}

impl CreateArgs {
    /// Run the project create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let owner_id = super::copy::resolve_owner_id(&client, &self.owner).await?;

        let query = r"
            mutation CreateProject($ownerId: ID!, $title: String!) {
                createProjectV2(input: { ownerId: $ownerId, title: $title }) {
                    projectV2 { id number title url }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("ownerId".to_string(), Value::String(owner_id));
        vars.insert("title".to_string(), Value::String(self.title.clone()));

        let result: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to create project")?;

        let number = result
            .pointer("/createProjectV2/projectV2/number")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let url = result
            .pointer("/createProjectV2/projectV2/url")
            .and_then(Value::as_str)
            .unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Created project #{number}: {}",
            cs.success_icon(),
            cs.bold(&self.title),
        );
        ios_println!(ios, "{url}");

        Ok(())
    }
}
