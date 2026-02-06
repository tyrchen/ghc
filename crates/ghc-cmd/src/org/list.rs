//! `ghc org list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List organizations for the authenticated user.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Maximum number of organizations to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the org list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the organizations cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let path = format!("user/orgs?per_page={}", self.limit.min(100));
        let orgs: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list organizations")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&orgs)?);
            return Ok(());
        }

        if orgs.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "You are not a member of any organizations");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for org in &orgs {
            let login = org.get("login").and_then(Value::as_str).unwrap_or("");
            let description = org.get("description").and_then(Value::as_str).unwrap_or("");
            let url = org.get("url").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                cs.bold(login),
                description.to_string(),
                url.to_string(),
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
    async fn test_should_list_organizations() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user/orgs",
            serde_json::json!([
                {"login": "my-org", "description": "My Organization", "url": "https://api.github.com/orgs/my-org"},
                {"login": "another-org", "description": "", "url": "https://api.github.com/orgs/another-org"}
            ]),
        )
        .await;

        let args = ListArgs {
            limit: 30,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("my-org"));
        assert!(out.contains("My Organization"));
        assert!(out.contains("another-org"));
    }

    #[tokio::test]
    async fn test_should_output_orgs_as_json() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user/orgs",
            serde_json::json!([
                {"login": "my-org", "description": "My Organization"}
            ]),
        )
        .await;

        let args = ListArgs {
            limit: 30,
            json: vec!["login".into()],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("\"login\""));
        assert!(out.contains("\"my-org\""));
    }
}
