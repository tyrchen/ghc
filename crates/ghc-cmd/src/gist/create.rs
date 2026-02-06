//! `ghc gist create` command.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Create a new gist.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Files to include in the gist (or `-` for stdin).
    #[arg(value_name = "FILE", required = true)]
    files: Vec<String>,

    /// Description for the gist.
    #[arg(short, long)]
    description: Option<String>,

    /// Create a public gist.
    #[arg(short, long)]
    public: bool,

    /// Open the gist in the browser after creation.
    #[arg(short, long)]
    web: bool,

    /// Filename to use when reading from stdin.
    #[arg(short, long, default_value = "gistfile.txt")]
    filename: String,
}

impl CreateArgs {
    /// Run the gist create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the gist cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let mut gist_files: HashMap<String, Value> = HashMap::new();

        for file_path in &self.files {
            if file_path == "-" {
                let mut content = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)
                    .context("failed to read from stdin")?;
                gist_files.insert(
                    self.filename.clone(),
                    serde_json::json!({ "content": content }),
                );
            } else {
                let content = std::fs::read_to_string(file_path)
                    .with_context(|| format!("failed to read file: {file_path}"))?;
                let name = Path::new(file_path)
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or(file_path)
                    .to_string();
                gist_files.insert(name, serde_json::json!({ "content": content }));
            }
        }

        let body = serde_json::json!({
            "description": self.description.as_deref().unwrap_or(""),
            "public": self.public,
            "files": gist_files,
        });

        let result: Value = client
            .rest(reqwest::Method::POST, "gists", Some(&body))
            .await
            .context("failed to create gist")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");
        let gist_id = result.get("id").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Created gist {gist_id}", cs.success_icon());
        ios_println!(ios, "{html_url}");

        if self.web && !html_url.is_empty() {
            factory.browser().open(html_url)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_gist_from_file() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/gists",
            201,
            serde_json::json!({
                "id": "new123",
                "html_url": "https://gist.github.com/new123",
            }),
        )
        .await;

        let tmp = std::env::temp_dir().join("ghc_test_gist_create.rs");
        std::fs::write(&tmp, "fn main() {}").unwrap();

        let args = CreateArgs {
            files: vec![tmp.to_string_lossy().into_owned()],
            description: Some("Test gist".into()),
            public: true,
            web: false,
            filename: "gistfile.txt".into(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created gist"));
        assert!(err.contains("new123"));
        let out = h.stdout();
        assert!(out.contains("https://gist.github.com/new123"));

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn test_should_fail_when_file_not_found() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            files: vec!["/nonexistent/path/file.rs".into()],
            description: None,
            public: false,
            web: false,
            filename: "gistfile.txt".into(),
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
