//! `ghc project field-delete` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Delete a field from a project.
#[derive(Debug, Args)]
pub struct FieldDeleteArgs {
    /// Field ID to delete.
    #[arg(value_name = "FIELD_ID")]
    field_id: String,
}

impl FieldDeleteArgs {
    /// Run the project field-delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the field cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let query = r"
            mutation DeleteField($fieldId: ID!) {
                deleteProjectV2Field(input: { fieldId: $fieldId }) {
                    projectV2Field {
                        ... on ProjectV2Field { id }
                        ... on ProjectV2SingleSelectField { id }
                    }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("fieldId".to_string(), Value::String(self.field_id.clone()));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to delete field")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Deleted field", cs.success_icon());

        Ok(())
    }
}
