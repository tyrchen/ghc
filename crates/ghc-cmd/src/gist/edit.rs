//! `ghc gist edit` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Edit a gist.
///
/// With no flags, opens gist files in your editor for interactive editing.
/// Use `--filename` to select a specific file to edit. Otherwise all files
/// are opened one at a time.
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

        // Determine if this is an interactive edit (no add/remove/description flags)
        let interactive =
            self.add.is_empty() && self.remove.is_empty() && self.description.is_none();

        let mut body = serde_json::json!({});

        if let Some(ref desc) = self.description {
            body["description"] = Value::String(desc.clone());
        }

        let mut files: HashMap<String, Value> = HashMap::new();

        if interactive {
            // Fetch gist to get current file contents
            let fetch_path = format!("gists/{gist_id}");
            let gist_data: Value = client
                .rest(reqwest::Method::GET, &fetch_path, None)
                .await
                .context("failed to fetch gist")?;

            let gist_files = gist_data
                .get("files")
                .and_then(Value::as_object)
                .ok_or_else(|| anyhow::anyhow!("gist has no files"))?;

            let prompter = factory.prompter();

            for (name, file_data) in gist_files {
                // If --filename is set, only edit that specific file
                if let Some(ref target) = self.filename
                    && target != name
                {
                    continue;
                }

                let current_content = file_data
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                let edited = prompter
                    .editor(
                        &format!("Editing {name}"),
                        current_content,
                        true, // allow blank
                    )
                    .with_context(|| format!("failed to edit file: {name}"))?;

                // Only include files that were actually changed
                if edited != current_content {
                    files.insert(name.clone(), serde_json::json!({ "content": edited }));
                }
            }

            if files.is_empty() {
                let ios = &factory.io;
                ios_eprintln!(ios, "No changes made");
                return Ok(());
            }
        } else {
            // Non-interactive: add/remove files
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

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_patch};

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

    #[tokio::test]
    async fn test_should_remove_file_from_gist() {
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
            description: None,
            filename: None,
            remove: vec!["old_file.txt".into()],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Updated gist"));
    }

    #[tokio::test]
    async fn test_should_edit_gist_interactively() {
        let h = TestHarness::new().await;

        // Mock fetching the gist
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "files": {
                    "hello.rs": {
                        "content": "fn main() {}"
                    }
                }
            }),
        )
        .await;

        // Mock updating the gist
        mock_rest_patch(
            &h.server,
            "/gists/abc123",
            200,
            serde_json::json!({
                "html_url": "https://gist.github.com/abc123",
            }),
        )
        .await;

        // Configure the stub prompter to return edited content
        h.prompter
            .input_answers
            .lock()
            .unwrap()
            .push("fn main() { println!(\"hello\"); }".into());

        let args = EditArgs {
            gist: "abc123".into(),
            add: vec![],
            description: None,
            filename: None,
            remove: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Updated gist"));
    }

    #[tokio::test]
    async fn test_should_skip_unchanged_files_in_interactive_edit() {
        let h = TestHarness::new().await;

        // Mock fetching the gist
        mock_rest_get(
            &h.server,
            "/gists/abc123",
            serde_json::json!({
                "files": {
                    "hello.rs": {
                        "content": "fn main() {}"
                    }
                }
            }),
        )
        .await;

        // StubPrompter editor returns default (unchanged content) when no answers configured

        let args = EditArgs {
            gist: "abc123".into(),
            add: vec![],
            description: None,
            filename: None,
            remove: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("No changes made"));
    }

    #[tokio::test]
    async fn test_should_extract_gist_id_from_url() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/gists/def456",
            200,
            serde_json::json!({
                "html_url": "https://gist.github.com/def456",
            }),
        )
        .await;

        let args = EditArgs {
            gist: "https://gist.github.com/user/def456".into(),
            add: vec![],
            description: Some("From URL".into()),
            filename: None,
            remove: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("gist.github.com/def456"));
    }
}
