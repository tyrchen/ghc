//! `ghc gist list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// List your gists.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Maximum number of gists to list.
    #[arg(short = 'L', long, default_value = "10")]
    limit: u32,

    /// Filter by visibility.
    #[arg(long, value_parser = ["public", "secret"])]
    visibility: Option<String>,
}

impl ListArgs {
    /// Run the gist list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the gists cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let path = format!("gists?per_page={}", self.limit.min(100));
        let gists: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list gists")?;

        if gists.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No gists found");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for gist in &gists {
            let id = gist.get("id").and_then(Value::as_str).unwrap_or("");
            let description = gist
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let is_public = gist.get("public").and_then(Value::as_bool).unwrap_or(false);
            let file_count = gist
                .get("files")
                .and_then(Value::as_object)
                .map_or(0, serde_json::Map::len);
            let updated_at = gist.get("updated_at").and_then(Value::as_str).unwrap_or("");

            // Apply visibility filter
            match self.visibility.as_deref() {
                Some("public") if !is_public => continue,
                Some("secret") if is_public => continue,
                _ => {}
            }

            let visibility = if is_public {
                cs.success("public")
            } else {
                cs.warning("secret")
            };

            let desc = if description.is_empty() {
                cs.gray("(no description)")
            } else {
                text::truncate(description, 60)
            };

            tp.add_row(vec![
                cs.bold(id),
                desc,
                visibility,
                format!("{file_count} file(s)"),
                updated_at.to_string(),
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
    async fn test_should_list_gists() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists",
            serde_json::json!([
                {
                    "id": "abc123",
                    "description": "My gist",
                    "public": true,
                    "files": {"test.rs": {}},
                    "updated_at": "2024-01-15T10:00:00Z"
                },
                {
                    "id": "def456",
                    "description": "Secret gist",
                    "public": false,
                    "files": {"notes.md": {}, "data.json": {}},
                    "updated_at": "2024-01-14T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            limit: 10,
            visibility: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("abc123"));
        assert!(out.contains("My gist"));
        assert!(out.contains("def456"));
    }

    #[tokio::test]
    async fn test_should_filter_gists_by_visibility() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists",
            serde_json::json!([
                {
                    "id": "abc123",
                    "description": "Public gist",
                    "public": true,
                    "files": {"test.rs": {}},
                    "updated_at": "2024-01-15T10:00:00Z"
                },
                {
                    "id": "def456",
                    "description": "Secret gist",
                    "public": false,
                    "files": {"notes.md": {}},
                    "updated_at": "2024-01-14T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            limit: 10,
            visibility: Some("public".into()),
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("abc123"));
        assert!(!out.contains("def456"));
    }
}
