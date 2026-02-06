//! `ghc cache list` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List GitHub Actions cache entries.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of caches to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Sort by field.
    #[arg(long, value_parser = ["created_at", "last_accessed_at", "size_in_bytes"])]
    sort: Option<String>,

    /// Sort order.
    #[arg(long, value_parser = ["asc", "desc"], default_value = "desc")]
    order: String,

    /// Filter by key prefix.
    #[arg(long)]
    key: Option<String>,

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
    /// Run the cache list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the caches cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let mut path = format!(
            "repos/{}/{}/actions/caches?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit.min(100),
        );
        if let Some(ref sort) = self.sort {
            let _ = write!(path, "&sort={sort}&direction={}", self.order);
        }
        if let Some(ref key) = self.key {
            let encoded = ghc_core::text::percent_encode(key);
            let _ = write!(path, "&key={encoded}");
        }

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list caches")?;

        // Extract inner array from wrapper object
        let items = result
            .get("actions_caches")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
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

        let caches = items
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if caches.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No caches found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for cache in caches {
            let id = cache.get("id").and_then(Value::as_u64).unwrap_or(0);
            let key = cache.get("key").and_then(Value::as_str).unwrap_or("");
            let size = cache
                .get("size_in_bytes")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let last_accessed = cache
                .get("last_accessed_at")
                .and_then(Value::as_str)
                .unwrap_or("");
            let ref_name = cache.get("ref").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                format!("{id}"),
                cs.bold(key),
                format_size(size),
                ref_name.to_string(),
                last_accessed.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

/// Format a byte size into a human-readable string.
#[allow(clippy::cast_precision_loss)]
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_caches() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/caches",
            serde_json::json!({
                "total_count": 1,
                "actions_caches": [
                    {
                        "id": 42,
                        "key": "rust-cache-main",
                        "ref": "refs/heads/main",
                        "size_in_bytes": 2_097_152,
                        "last_accessed_at": "2024-01-15T10:00:00Z"
                    }
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            limit: 30,
            sort: None,
            order: "desc".to_string(),
            key: None,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(
            stdout.contains("rust-cache-main"),
            "should contain cache key"
        );
        assert!(stdout.contains("2.0 MB"), "should contain formatted size");
    }

    #[test]
    fn test_should_format_size_correctly() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }
}
