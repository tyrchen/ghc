//! `ghc extension browse` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Browse popular GitHub CLI extensions.
#[derive(Debug, Args)]
pub struct BrowseArgs {
    /// Open the extensions marketplace in the browser.
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

impl BrowseArgs {
    /// Run the extension browse command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extensions cannot be browsed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        if self.web {
            factory
                .browser()
                .open("https://github.com/topics/gh-extension")?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = "search/repositories?q=topic:gh-extension&sort=stars&order=desc&per_page=30";

        let result: Value = client
            .rest(reqwest::Method::GET, path, None)
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
            ios_eprintln!(ios, "No extensions found");
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

            tp.add_row(vec![
                cs.bold(full_name),
                description.to_string(),
                format!("*{stars}"),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_browse_extensions() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/repositories",
            serde_json::json!({
                "total_count": 2,
                "items": [
                    {
                        "full_name": "dlvhdr/gh-dash",
                        "description": "A beautiful dashboard for GitHub",
                        "stargazers_count": 5000
                    },
                    {
                        "full_name": "vilmibm/gh-screensaver",
                        "description": "A terminal screensaver",
                        "stargazers_count": 300
                    }
                ]
            }),
        )
        .await;

        let args = BrowseArgs {
            web: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("gh-dash"), "should contain extension name");
        assert!(
            stdout.contains("gh-screensaver"),
            "should contain second extension"
        );
        assert!(stdout.contains("*5000"), "should contain star count");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let args = BrowseArgs {
            web: true,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("gh-extension"));
    }
}
