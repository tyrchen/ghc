//! `ghc project item-delete` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Delete a project item.
#[derive(Debug, Args)]
pub struct ItemDeleteArgs {
    /// Item ID to delete.
    #[arg(value_name = "ITEM_ID")]
    item_id: String,

    /// Project number.
    #[arg(long)]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,
}

impl ItemDeleteArgs {
    /// Run the project item-delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the item cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation DeleteItem($projectId: ID!, $itemId: ID!) {
                deleteProjectV2Item(input: {
                    projectId: $projectId,
                    itemId: $itemId
                }) {
                    deletedItemId
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("itemId".to_string(), Value::String(self.item_id.clone()));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to delete item")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Deleted item from project", cs.success_icon());

        Ok(())
    }
}
