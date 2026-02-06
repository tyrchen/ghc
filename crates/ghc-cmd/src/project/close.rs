//! `ghc project close` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Close a project.
#[derive(Debug, Args)]
pub struct CloseArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,
}

impl CloseArgs {
    /// Run the project close command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be closed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id = resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation CloseProject($projectId: ID!) {
                updateProjectV2(input: { projectId: $projectId, closed: true }) {
                    projectV2 { id title }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to close project")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Closed project #{}", cs.success_icon(), self.number,);

        Ok(())
    }
}

/// Resolve a project's node ID from the owner login and project number.
pub(super) async fn resolve_project_id(
    client: &ghc_api::client::Client,
    owner: &str,
    number: u32,
) -> Result<String> {
    let query = r"
        query FindProject($owner: String!, $number: Int!) {
            user(login: $owner) {
                projectV2(number: $number) { id }
            }
        }
    ";

    let mut vars = HashMap::new();
    vars.insert("owner".to_string(), Value::String(owner.to_string()));
    vars.insert(
        "number".to_string(),
        Value::Number(serde_json::Number::from(number)),
    );

    let data: Value = client
        .graphql(query, &vars)
        .await
        .context("failed to find project")?;

    // Try user first, then org
    if let Some(id) = data.pointer("/user/projectV2/id").and_then(Value::as_str) {
        return Ok(id.to_string());
    }

    let org_query = r"
        query FindOrgProject($owner: String!, $number: Int!) {
            organization(login: $owner) {
                projectV2(number: $number) { id }
            }
        }
    ";

    let org_data: Value = client
        .graphql(org_query, &vars)
        .await
        .context("failed to find project in organization")?;

    org_data
        .pointer("/organization/projectV2/id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("project #{number} not found for {owner}"))
}
