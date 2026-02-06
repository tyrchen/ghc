//! `ghc project item-create` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Create a draft issue in a project.
#[derive(Debug, Args)]
pub struct ItemCreateArgs {
    /// Title for the draft issue.
    #[arg(value_name = "TITLE")]
    title: String,

    /// Project number.
    #[arg(long)]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Body text for the draft issue.
    #[arg(long)]
    body: Option<String>,
}

impl ItemCreateArgs {
    /// Run the project item-create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the draft issue cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation CreateDraftIssue($projectId: ID!, $title: String!, $body: String) {
                addProjectV2DraftIssue(input: {
                    projectId: $projectId,
                    title: $title,
                    body: $body
                }) {
                    projectItem { id }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("title".to_string(), Value::String(self.title.clone()));
        if let Some(body) = &self.body {
            vars.insert("body".to_string(), Value::String(body.clone()));
        }

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to create draft issue")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Created draft issue in project #{}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}
