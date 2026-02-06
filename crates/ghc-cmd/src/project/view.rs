//! `ghc project view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_println;
use serde_json::Value;

/// View a project.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Open the project in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ViewArgs {
    /// Run the project view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be viewed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let query = r"
            query ViewProject($owner: String!, $number: Int!) {
                user(login: $owner) {
                    projectV2(number: $number) {
                        title shortDescription url closed readme
                        items(first: 0) { totalCount }
                        fields(first: 0) { totalCount }
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
            .context("failed to view project")?;

        let project = data
            .pointer("/user/projectV2")
            .cloned()
            .unwrap_or(Value::Null);

        // If user query returned null, try organization
        let project = if project.is_null() {
            let org_query = r"
                query ViewOrgProject($owner: String!, $number: Int!) {
                    organization(login: $owner) {
                        projectV2(number: $number) {
                            title shortDescription url closed readme
                            items(first: 0) { totalCount }
                            fields(first: 0) { totalCount }
                        }
                    }
                }
            ";

            let org_data: Value = client
                .graphql(org_query, &vars)
                .await
                .context("failed to view organization project")?;

            org_data
                .pointer("/organization/projectV2")
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("project #{} not found for {}", self.number, self.owner,)
                })?
        } else {
            project
        };

        let url = project.get("url").and_then(Value::as_str).unwrap_or("");

        if self.web {
            factory.browser().open(url)?;
            return Ok(());
        }

        let ios = &factory.io;

        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &project,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let cs = ios.color_scheme();

        let title = project.get("title").and_then(Value::as_str).unwrap_or("");
        let description = project
            .get("shortDescription")
            .and_then(Value::as_str)
            .unwrap_or("");
        let closed = project
            .get("closed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let readme = project.get("readme").and_then(Value::as_str).unwrap_or("");
        let item_count = project
            .pointer("/items/totalCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let field_count = project
            .pointer("/fields/totalCount")
            .and_then(Value::as_u64)
            .unwrap_or(0);

        let status = if closed {
            cs.gray("closed")
        } else {
            cs.success("open")
        };

        ios_println!(ios, "{}", cs.bold(title));
        ios_println!(ios, "#{} - {status}", self.number);

        if !description.is_empty() {
            ios_println!(ios, "\n{description}");
        }

        ios_println!(ios, "\nItems: {item_count}");
        ios_println!(ios, "Fields: {field_count}");
        ios_println!(ios, "URL: {url}");

        if !readme.is_empty() {
            ios_println!(ios, "\n--- README ---\n{readme}");
        }

        Ok(())
    }
}
