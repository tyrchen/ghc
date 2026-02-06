//! `ghc project item-add` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Add an item (issue or pull request) to a project.
#[derive(Debug, Args)]
pub struct ItemAddArgs {
    /// URL of the issue or pull request to add.
    #[arg(value_name = "URL")]
    url: String,

    /// Project number.
    #[arg(long)]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,
}

impl ItemAddArgs {
    /// Run the project item-add command.
    ///
    /// # Errors
    ///
    /// Returns an error if the item cannot be added.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        // Resolve content ID from URL
        let content_id = resolve_content_id(&client, &self.url).await?;

        let query = r"
            mutation AddItem($projectId: ID!, $contentId: ID!) {
                addProjectV2ItemById(input: {
                    projectId: $projectId,
                    contentId: $contentId
                }) {
                    item { id }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("contentId".to_string(), Value::String(content_id));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to add item to project")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Added item to project #{}",
            cs.success_icon(),
            self.number,
        );

        Ok(())
    }
}

/// Resolve a content node ID from a GitHub URL (issue or PR).
async fn resolve_content_id(client: &ghc_api::client::Client, url: &str) -> Result<String> {
    let query = r"
        query ResolveNode($url: URI!) {
            resource(url: $url) {
                ... on Issue { id }
                ... on PullRequest { id }
            }
        }
    ";

    let mut vars = HashMap::new();
    vars.insert("url".to_string(), Value::String(url.to_string()));

    let data: Value = client
        .graphql(query, &vars)
        .await
        .context("failed to resolve URL")?;

    data.pointer("/resource/id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("could not resolve URL: {url}"))
}
