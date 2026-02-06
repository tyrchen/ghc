//! `ghc label list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// List labels in a repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of labels to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the label list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the labels cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let path = format!(
            "repos/{}/{}/labels?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit.min(100),
        );

        let labels: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list labels")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&labels)?);
            return Ok(());
        }

        if labels.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No labels found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for label in &labels {
            let name = label.get("name").and_then(Value::as_str).unwrap_or("");
            let description = label
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let color = label.get("color").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                cs.bold(name),
                text::truncate(description, 50),
                format!("#{color}"),
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
    async fn test_should_list_labels() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/labels",
            serde_json::json!([
                {"name": "bug", "color": "d73a4a", "description": "Something isn't working"},
                {"name": "enhancement", "color": "a2eeef", "description": "New feature"}
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("bug"));
        assert!(out.contains("enhancement"));
        assert!(out.contains("#d73a4a"));
    }

    #[tokio::test]
    async fn test_should_output_labels_as_json() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/labels",
            serde_json::json!([
                {"name": "bug", "color": "d73a4a", "description": "Something isn't working"}
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            json: vec!["name".into()],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("\"name\""));
        assert!(out.contains("\"bug\""));
    }

    #[tokio::test]
    async fn test_should_fail_without_repo_flag() {
        let h = TestHarness::new().await;

        let args = ListArgs {
            repo: None,
            limit: 30,
            json: vec![],
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
