//! `ghc gist rename` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Rename a file in a gist.
#[derive(Debug, Args)]
pub struct RenameArgs {
    /// The gist ID or URL.
    #[arg(value_name = "GIST")]
    gist: String,

    /// The current filename.
    #[arg(value_name = "OLD_NAME")]
    old_name: String,

    /// The new filename.
    #[arg(value_name = "NEW_NAME")]
    new_name: String,
}

impl RenameArgs {
    /// Run the gist rename command.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be renamed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let gist_id = self.gist.rsplit('/').next().unwrap_or(&self.gist);

        let client = factory.api_client("github.com")?;

        // First fetch the gist to get the current file content
        let get_path = format!("gists/{gist_id}");
        let gist_data: Value = client
            .rest(reqwest::Method::GET, &get_path, None)
            .await
            .context("failed to fetch gist")?;

        let content = gist_data
            .pointer(&format!("/files/{}/content", self.old_name))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("file '{}' not found in gist {gist_id}", self.old_name))?
            .to_string();

        // Rename by creating new file and deleting old one
        let body = serde_json::json!({
            "files": {
                self.old_name.clone(): Value::Null,
                self.new_name.clone(): { "content": content },
            }
        });

        let result: Value = client
            .rest(reqwest::Method::PATCH, &get_path, Some(&body))
            .await
            .context("failed to rename file in gist")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Renamed '{}' to '{}' in gist {gist_id}",
            cs.success_icon(),
            self.old_name,
            self.new_name,
        );
        ios_println!(ios, "{html_url}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_patch};

    #[tokio::test]
    async fn test_should_rename_gist_file() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "id": "abc123",
                "files": {
                    "old_name.rs": { "content": "fn main() {}" }
                }
            }),
        )
        .await;
        mock_rest_patch(
            &h.server,
            "/gists/abc123",
            200,
            serde_json::json!({
                "html_url": "https://gist.github.com/abc123",
            }),
        )
        .await;

        let args = RenameArgs {
            gist: "abc123".into(),
            old_name: "old_name.rs".into(),
            new_name: "new_name.rs".into(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Renamed"));
        assert!(err.contains("old_name.rs"));
        assert!(err.contains("new_name.rs"));
    }

    #[tokio::test]
    async fn test_should_fail_when_file_not_found_in_gist() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "id": "abc123",
                "files": {
                    "other.rs": { "content": "fn other() {}" }
                }
            }),
        )
        .await;

        let args = RenameArgs {
            gist: "abc123".into(),
            old_name: "nonexistent.rs".into(),
            new_name: "new_name.rs".into(),
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent.rs"));
    }
}
