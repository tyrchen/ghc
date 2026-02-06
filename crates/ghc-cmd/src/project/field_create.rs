//! `ghc project field-create` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Create a field in a project.
#[derive(Debug, Args)]
pub struct FieldCreateArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Name for the new field.
    #[arg(long)]
    name: String,

    /// Data type: TEXT, NUMBER, DATE, SINGLE_SELECT, ITERATION.
    #[arg(long)]
    data_type: String,

    /// Options for SINGLE_SELECT fields (comma-separated).
    #[arg(long, value_delimiter = ',')]
    single_select_options: Vec<String>,
}

impl FieldCreateArgs {
    /// Run the project field-create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the field cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation CreateField(
                $projectId: ID!,
                $name: String!,
                $dataType: ProjectV2CustomFieldType!,
                $singleSelectOptions: [ProjectV2SingleSelectFieldOptionInput!]
            ) {
                createProjectV2Field(input: {
                    projectId: $projectId,
                    dataType: $dataType,
                    name: $name,
                    singleSelectOptions: $singleSelectOptions
                }) {
                    projectV2Field {
                        ... on ProjectV2SingleSelectField { id name }
                        ... on ProjectV2Field { id name }
                    }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));
        vars.insert("name".to_string(), Value::String(self.name.clone()));
        vars.insert(
            "dataType".to_string(),
            Value::String(self.data_type.to_uppercase()),
        );

        if !self.single_select_options.is_empty() {
            let options: Vec<Value> = self
                .single_select_options
                .iter()
                .map(|opt| {
                    let mut map = serde_json::Map::new();
                    map.insert("name".to_string(), Value::String(opt.clone()));
                    map.insert("color".to_string(), Value::String("GRAY".to_string()));
                    map.insert("description".to_string(), Value::String(String::new()));
                    Value::Object(map)
                })
                .collect();
            vars.insert("singleSelectOptions".to_string(), Value::Array(options));
        }

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to create field")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Created field {} in project #{}",
            cs.success_icon(),
            cs.bold(&self.name),
            self.number,
        );

        Ok(())
    }
}
