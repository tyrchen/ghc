//! `ghc project field-list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List fields in a project.
#[derive(Debug, Args)]
pub struct FieldListArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl FieldListArgs {
    /// Run the project field-list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the fields cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let query = r"
            query ListFields($owner: String!, $number: Int!) {
                user(login: $owner) {
                    projectV2(number: $number) {
                        fields(first: 100) {
                            nodes {
                                ... on ProjectV2Field {
                                    id name dataType
                                }
                                ... on ProjectV2SingleSelectField {
                                    id name dataType
                                    options { id name }
                                }
                                ... on ProjectV2IterationField {
                                    id name dataType
                                }
                            }
                        }
                    }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("owner".to_string(), Value::String(self.owner.clone()));
        vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to list fields")?;

        // Try user path first, then org
        let fields = data
            .pointer("/user/projectV2/fields/nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // If user didn't return results, try org query
        let fields = if fields.is_empty() {
            let org_query = r"
                query ListOrgFields($owner: String!, $number: Int!) {
                    organization(login: $owner) {
                        projectV2(number: $number) {
                            fields(first: 100) {
                                nodes {
                                    ... on ProjectV2Field {
                                        id name dataType
                                    }
                                    ... on ProjectV2SingleSelectField {
                                        id name dataType
                                        options { id name }
                                    }
                                    ... on ProjectV2IterationField {
                                        id name dataType
                                    }
                                }
                            }
                        }
                    }
                }
            ";

            let org_data: Value = client
                .graphql(org_query, &vars)
                .await
                .context("failed to list fields from organization")?;

            org_data
                .pointer("/organization/projectV2/fields/nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        } else {
            fields
        };

        let ios = &factory.io;
        let cs = ios.color_scheme();

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&fields)?);
            return Ok(());
        }

        if fields.is_empty() {
            ios_eprintln!(ios, "No fields found in project #{}", self.number);
            return Ok(());
        }
        let mut tp = TablePrinter::new(ios);

        for field in &fields {
            let name = field.get("name").and_then(Value::as_str).unwrap_or("");
            let data_type = field.get("dataType").and_then(Value::as_str).unwrap_or("");
            let id = field.get("id").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![cs.bold(name), data_type.to_string(), cs.gray(id)]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}
