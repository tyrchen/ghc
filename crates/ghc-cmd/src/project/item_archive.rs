//! `ghc project item-archive` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Archive a project item.
#[derive(Debug, Args)]
pub struct ItemArchiveArgs {
    /// Item ID to archive.
    #[arg(value_name = "ITEM_ID")]
    item_id: String,

    /// Project number.
    #[arg(long)]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Undo: unarchive the item instead.
    #[arg(long)]
    undo: bool,
}

impl ItemArchiveArgs {
    /// Run the project item-archive command.
    ///
    /// # Errors
    ///
    /// Returns an error if the item cannot be archived.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        if self.undo {
            let query = r"
                mutation UnarchiveItem($projectId: ID!, $itemId: ID!) {
                    unarchiveProjectV2Item(input: {
                        projectId: $projectId,
                        itemId: $itemId
                    }) {
                        item { id }
                    }
                }
            ";

            let mut vars = HashMap::new();
            vars.insert("projectId".to_string(), Value::String(project_id));
            vars.insert("itemId".to_string(), Value::String(self.item_id.clone()));

            let _: Value = client
                .graphql(query, &vars)
                .await
                .context("failed to unarchive item")?;

            ios_eprintln!(ios, "{} Unarchived item", cs.success_icon());
        } else {
            let query = r"
                mutation ArchiveItem($projectId: ID!, $itemId: ID!) {
                    archiveProjectV2Item(input: {
                        projectId: $projectId,
                        itemId: $itemId
                    }) {
                        item { id }
                    }
                }
            ";

            let mut vars = HashMap::new();
            vars.insert("projectId".to_string(), Value::String(project_id));
            vars.insert("itemId".to_string(), Value::String(self.item_id.clone()));

            let _: Value = client
                .graphql(query, &vars)
                .await
                .context("failed to archive item")?;

            ios_eprintln!(ios, "{} Archived item", cs.success_icon());
        }

        Ok(())
    }
}
