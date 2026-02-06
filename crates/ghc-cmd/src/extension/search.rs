//! `ghc extension search` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Search for GitHub CLI extensions.
#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query.
    #[arg(value_name = "QUERY")]
    query: String,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Open search results in the browser.
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

impl SearchArgs {
    /// Run the extension search command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        if self.web {
            let encoded = ghc_core::text::percent_encode(&self.query);
            let url = format!(
                "https://github.com/search?q={encoded}+topic:gh-extension&type=repositories"
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let encoded_query = ghc_core::text::percent_encode(&self.query);
        let path = format!(
            "search/repositories?q={encoded_query}+topic:gh-extension&sort=stars&order=desc&per_page={}",
            self.limit,
        );

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search extensions")?;

        let items = result
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let arr = Value::Array(items.clone());
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if items.is_empty() {
            ios_eprintln!(ios, "No extensions found matching \"{}\"", self.query);
            return Ok(());
        }
        let mut tp = TablePrinter::new(ios);

        for item in &items {
            let full_name = item.get("full_name").and_then(Value::as_str).unwrap_or("");
            let description = item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let stars = item
                .get("stargazers_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let updated = item.get("updated_at").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                cs.bold(full_name),
                description.to_string(),
                format!("*{stars}"),
                cs.gray(updated),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}
