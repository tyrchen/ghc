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

    /// Include file content in the output.
    #[arg(long)]
    include_content: bool,

    /// Filter gists by regex matching description or filenames.
    #[arg(long, value_name = "PATTERN")]
    filter: Option<String>,

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

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let items = Value::Array(gists.clone());
            let output = ghc_core::json::format_json_output(
                &items,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        // Compile regex filter if provided
        let filter_regex = if let Some(ref pattern) = self.filter {
            Some(regex::Regex::new(pattern).context("invalid filter regex pattern")?)
        } else {
            None
        };

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for gist in &gists {
            let id = gist.get("id").and_then(Value::as_str).unwrap_or("");
            let description = gist
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let is_public = gist.get("public").and_then(Value::as_bool).unwrap_or(false);
            let files = gist.get("files").and_then(Value::as_object);
            let file_count = files.map_or(0, serde_json::Map::len);
            let updated_at = gist.get("updated_at").and_then(Value::as_str).unwrap_or("");

            // Apply visibility filter
            match self.visibility.as_deref() {
                Some("public") if !is_public => continue,
                Some("secret") if is_public => continue,
                _ => {}
            }

            // Apply regex filter
            if let Some(ref re) = filter_regex {
                let desc_matches = re.is_match(description);
                let filename_matches =
                    files.is_some_and(|f| f.keys().any(|name| re.is_match(name)));
                if !desc_matches && !filename_matches {
                    continue;
                }
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

        if self.include_content {
            print_gist_contents(ios, &gists);
        }

        Ok(())
    }
}

/// Print gist file contents to stdout.
fn print_gist_contents(ios: &ghc_core::iostreams::IOStreams, gists: &[Value]) {
    let cs = ios.color_scheme();
    for gist in gists {
        let files = gist.get("files").and_then(Value::as_object);
        if let Some(files) = files {
            for (name, file_data) in files {
                let content = file_data
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !content.is_empty() {
                    ios_println!(ios, "\n{}", cs.bold(name));
                    ios_println!(ios, "{content}");
                }
            }
        }
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
            include_content: false,
            filter: None,
            json: vec![],
            jq: None,
            template: None,
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
            include_content: false,
            filter: None,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("abc123"));
        assert!(!out.contains("def456"));
    }

    #[tokio::test]
    async fn test_should_filter_gists_by_regex() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/gists",
            serde_json::json!([
                {
                    "id": "abc123",
                    "description": "Rust utilities",
                    "public": true,
                    "files": {"utils.rs": {}},
                    "updated_at": "2024-01-15T10:00:00Z"
                },
                {
                    "id": "def456",
                    "description": "Python scripts",
                    "public": true,
                    "files": {"script.py": {}},
                    "updated_at": "2024-01-14T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            limit: 10,
            visibility: None,
            include_content: false,
            filter: Some("(?i)rust".into()),
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("abc123"));
        assert!(!out.contains("def456"));
    }
}
