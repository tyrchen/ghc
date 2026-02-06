//! `ghc gist view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;

/// View a gist.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// The gist ID or URL to view.
    #[arg(value_name = "GIST")]
    gist: String,

    /// View a specific file in the gist.
    #[arg(short, long, value_name = "FILENAME")]
    filename: Option<String>,

    /// Display raw file content without decoration.
    #[arg(short, long)]
    raw: bool,

    /// Open the gist in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the gist view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the gist cannot be viewed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let gist_id = self.gist.rsplit('/').next().unwrap_or(&self.gist);

        if self.web {
            let url = format!("https://gist.github.com/{gist_id}");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;

        let path = format!("gists/{gist_id}");
        let gist: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch gist")?;

        // JSON output
        let ios = &factory.io;
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&gist)?);
            return Ok(());
        }

        let description = gist
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let files = gist
            .get("files")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow::anyhow!("unexpected gist response format"))?;

        let cs = ios.color_scheme();

        if !self.raw {
            if !description.is_empty() {
                ios_println!(ios, "{}", cs.bold(description));
            }
            ios_println!(ios, "");
        }

        for (name, file_data) in files {
            if let Some(ref filter) = self.filename
                && name != filter
            {
                continue;
            }

            let content = file_data
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("");

            if self.raw {
                ios_println!(ios, "{content}");
            } else {
                ios_println!(ios, "{}", cs.cyan(name));
                ios_println!(ios, "");
                ios_println!(ios, "{content}");
                ios_println!(ios, "");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_view_gist() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "id": "abc123",
                "description": "My test gist",
                "files": {
                    "hello.rs": {
                        "content": "fn main() { println!(\"hello\"); }"
                    }
                }
            }),
        )
        .await;

        let args = ViewArgs {
            gist: "abc123".into(),
            filename: None,
            raw: false,
            web: false,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("My test gist"));
        assert!(out.contains("hello.rs"));
        assert!(out.contains("fn main()"));
    }

    #[tokio::test]
    async fn test_should_view_gist_raw() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "id": "abc123",
                "description": "My test gist",
                "files": {
                    "hello.rs": {
                        "content": "fn main() {}"
                    }
                }
            }),
        )
        .await;

        let args = ViewArgs {
            gist: "abc123".into(),
            filename: None,
            raw: true,
            web: false,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("fn main() {}"));
        assert!(!out.contains("My test gist"));
    }

    #[tokio::test]
    async fn test_should_open_gist_in_browser() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            gist: "abc123".into(),
            filename: None,
            raw: false,
            web: true,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("abc123"));
    }
}
