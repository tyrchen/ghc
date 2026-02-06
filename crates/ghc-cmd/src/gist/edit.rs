//! `ghc gist edit` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Edit a gist.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// The gist ID or URL to edit.
    #[arg(value_name = "GIST")]
    gist: String,

    /// Add a file to the gist.
    #[arg(short, long, value_name = "FILE")]
    add: Vec<String>,

    /// Update the gist description.
    #[arg(short, long)]
    description: Option<String>,

    /// Name of the file within the gist to edit.
    #[arg(short, long, value_name = "FILENAME")]
    filename: Option<String>,

    /// Remove a file from the gist.
    #[arg(short, long, value_name = "FILENAME")]
    remove: Vec<String>,
}

impl EditArgs {
    /// Run the gist edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the gist cannot be edited.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let gist_id = self.gist.rsplit('/').next().unwrap_or(&self.gist);

        let client = factory.api_client("github.com")?;

        let mut body = serde_json::json!({});

        if let Some(ref desc) = self.description {
            body["description"] = Value::String(desc.clone());
        }

        let mut files: HashMap<String, Value> = HashMap::new();

        // Add new files
        for file_path in &self.add {
            let content = std::fs::read_to_string(file_path)
                .with_context(|| format!("failed to read file: {file_path}"))?;
            let name = std::path::Path::new(file_path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or(file_path)
                .to_string();
            files.insert(name, serde_json::json!({ "content": content }));
        }

        // Remove files (set to null)
        for filename in &self.remove {
            files.insert(filename.clone(), Value::Null);
        }

        if !files.is_empty() {
            body["files"] = serde_json::to_value(&files).context("failed to serialize files")?;
        }

        let path = format!("gists/{gist_id}");
        let result: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to edit gist")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Updated gist {gist_id}", cs.success_icon());
        ios_println!(ios, "{html_url}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_edit_gist_description() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/gists/abc123",
            200,
            serde_json::json!({
                "html_url": "https://gist.github.com/abc123",
            }),
        )
        .await;

        let args = EditArgs {
            gist: "abc123".into(),
            add: vec![],
            description: Some("Updated description".into()),
            filename: None,
            remove: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Updated gist"));
        assert!(err.contains("abc123"));
    }
}
