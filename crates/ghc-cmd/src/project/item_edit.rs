//! `ghc project item-edit` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Edit a project item field value.
#[derive(Debug, Args)]
pub struct ItemEditArgs {
    /// Item ID to edit.
    #[arg(value_name = "ITEM_ID")]
    item_id: String,

    /// Project number.
    #[arg(long)]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Field ID to update.
    #[arg(long)]
    field_id: String,

    /// Text value to set.
    #[arg(long, group = "value")]
    text: Option<String>,

    /// Number value to set.
    #[arg(long, group = "value")]
    number_value: Option<f64>,

    /// Date value to set (ISO 8601 format).
    #[arg(long, group = "value")]
    date: Option<String>,

    /// Single select option ID to set.
    #[arg(long, group = "value")]
    single_select_option_id: Option<String>,

    /// Iteration ID to set.
    #[arg(long, group = "value")]
    iteration_id: Option<String>,

    /// Clear the field value.
    #[arg(long, group = "value")]
    clear: bool,
}

impl ItemEditArgs {
    /// Run the project item-edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the item field cannot be edited.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        if self.clear {
            return self.clear_field(&client, &project_id, factory).await;
        }

        let query = r"
            mutation EditItemField(
                $projectId: ID!,
                $itemId: ID!,
                $fieldId: ID!,
                $value: ProjectV2FieldValue!
            ) {
                updateProjectV2ItemFieldValue(input: {
                    projectId: $projectId,
                    itemId: $itemId,
                    fieldId: $fieldId,
                    value: $value
                }) {
                    projectV2Item { id }
                }
            }
        ";

        let field_value = self.build_field_value()?;

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("itemId".to_string(), Value::String(self.item_id.clone()));
        vars.insert("fieldId".to_string(), Value::String(self.field_id.clone()));
        vars.insert("value".to_string(), field_value);

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to edit item field")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Updated item field", cs.success_icon());

        Ok(())
    }

    /// Build the field value object for the GraphQL mutation.
    fn build_field_value(&self) -> Result<Value> {
        let mut value = serde_json::Map::new();

        if let Some(text) = &self.text {
            value.insert("text".to_string(), Value::String(text.clone()));
        } else if let Some(num) = self.number_value {
            value.insert(
                "number".to_string(),
                Value::Number(
                    serde_json::Number::from_f64(num)
                        .ok_or_else(|| anyhow::anyhow!("invalid number value"))?,
                ),
            );
        } else if let Some(date) = &self.date {
            value.insert("date".to_string(), Value::String(date.clone()));
        } else if let Some(opt_id) = &self.single_select_option_id {
            value.insert(
                "singleSelectOptionId".to_string(),
                Value::String(opt_id.clone()),
            );
        } else if let Some(iter_id) = &self.iteration_id {
            value.insert("iterationId".to_string(), Value::String(iter_id.clone()));
        } else {
            return Err(anyhow::anyhow!(
                "one of --text, --number-value, --date, --single-select-option-id, \
                 --iteration-id, or --clear must be specified"
            ));
        }

        Ok(Value::Object(value))
    }

    /// Clear a field value on an item.
    async fn clear_field(
        &self,
        client: &ghc_api::client::Client,
        project_id: &str,
        factory: &crate::factory::Factory,
    ) -> Result<()> {
        let query = r"
            mutation ClearItemField($projectId: ID!, $itemId: ID!, $fieldId: ID!) {
                clearProjectV2ItemFieldValue(input: {
                    projectId: $projectId,
                    itemId: $itemId,
                    fieldId: $fieldId
                }) {
                    projectV2Item { id }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert(
            "projectId".to_string(),
            Value::String(project_id.to_string()),
        );
        vars.insert("itemId".to_string(), Value::String(self.item_id.clone()));
        vars.insert("fieldId".to_string(), Value::String(self.field_id.clone()));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to clear item field")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Cleared item field", cs.success_icon());

        Ok(())
    }
}
