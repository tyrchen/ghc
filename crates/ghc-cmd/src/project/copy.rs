//! `ghc project copy` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::{ios_eprintln, ios_println};
use serde_json::Value;

/// Copy a project.
#[derive(Debug, Args)]
pub struct CopyArgs {
    /// Project number to copy.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the source project.
    #[arg(long)]
    source_owner: String,

    /// Owner for the new copy.
    #[arg(long)]
    target_owner: String,

    /// Title for the copied project.
    #[arg(short, long)]
    title: String,

    /// Include draft issues in the copy.
    #[arg(long)]
    drafts: bool,
}

impl CopyArgs {
    /// Run the project copy command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be copied.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.source_owner, self.number).await?;

        let query = r"
            mutation CopyProject($projectId: ID!, $ownerId: ID!, $title: String!, $includeDraftIssues: Boolean!) {
                copyProjectV2(input: {
                    projectId: $projectId,
                    ownerId: $ownerId,
                    title: $title,
                    includeDraftIssues: $includeDraftIssues
                }) {
                    projectV2 { id number title url }
                }
            }
        ";

        // Resolve target owner ID
        let owner_id = resolve_owner_id(&client, &self.target_owner).await?;

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("ownerId".to_string(), Value::String(owner_id));
        vars.insert("title".to_string(), Value::String(self.title.clone()));
        vars.insert("includeDraftIssues".to_string(), Value::Bool(self.drafts));

        let result: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to copy project")?;

        let new_number = result
            .pointer("/copyProjectV2/projectV2/number")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let url = result
            .pointer("/copyProjectV2/projectV2/url")
            .and_then(Value::as_str)
            .unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Copied project to #{new_number}", cs.success_icon(),);
        ios_println!(ios, "{url}");

        Ok(())
    }
}

/// Resolve an owner's node ID from their login.
pub(super) async fn resolve_owner_id(
    client: &ghc_api::client::Client,
    owner: &str,
) -> Result<String> {
    let query = r"
        query FindOwner($login: String!) {
            user(login: $login) { id }
            organization(login: $login) { id }
        }
    ";

    let mut vars = HashMap::new();
    vars.insert("login".to_string(), Value::String(owner.to_string()));

    let data: Value = client
        .graphql(query, &vars)
        .await
        .context("failed to resolve owner")?;

    data.pointer("/user/id")
        .or_else(|| data.pointer("/organization/id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("owner not found: {owner}"))
}
