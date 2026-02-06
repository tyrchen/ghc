//! `ghc project item-list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List items in a project.
#[derive(Debug, Args)]
pub struct ItemListArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Maximum number of items to fetch.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ItemListArgs {
    /// Run the project item-list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the items cannot be listed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let query = r"
            query ListItems($owner: String!, $number: Int!, $first: Int!) {
                user(login: $owner) {
                    projectV2(number: $number) {
                        items(first: $first) {
                            nodes {
                                id type
                                content {
                                    ... on Issue { title number url state }
                                    ... on PullRequest { title number url state }
                                    ... on DraftIssue { title body }
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
        vars.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(self.limit)),
        );

        let data: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to list items")?;

        let items = data
            .pointer("/user/projectV2/items/nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // If user query returned no items, try organization
        let items = if items.is_empty() {
            let org_query = r"
                query ListOrgItems($owner: String!, $number: Int!, $first: Int!) {
                    organization(login: $owner) {
                        projectV2(number: $number) {
                            items(first: $first) {
                                nodes {
                                    id type
                                    content {
                                        ... on Issue { title number url state }
                                        ... on PullRequest { title number url state }
                                        ... on DraftIssue { title body }
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
                .context("failed to list items from organization")?;

            org_data
                .pointer("/organization/projectV2/items/nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        } else {
            items
        };

        let ios = &factory.io;

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&items)?);
            return Ok(());
        }

        if items.is_empty() {
            ios_eprintln!(ios, "No items found in project #{}", self.number);
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in &items {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");

            let content = item.get("content").cloned().unwrap_or(Value::Null);

            let title = content
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("(untitled)");
            let number = content
                .get("number")
                .and_then(Value::as_u64)
                .map(|n| format!("#{n}"))
                .unwrap_or_default();
            let state = content.get("state").and_then(Value::as_str).unwrap_or("");

            let state_display = match state {
                "OPEN" => cs.success("OPEN"),
                "CLOSED" => cs.error("CLOSED"),
                "MERGED" => cs.magenta("MERGED"),
                _ => state.to_string(),
            };

            tp.add_row(vec![
                item_type.to_string(),
                number,
                cs.bold(title),
                state_display,
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}
