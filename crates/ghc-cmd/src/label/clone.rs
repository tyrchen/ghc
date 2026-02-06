//! `ghc label clone` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Clone labels from another repository.
#[derive(Debug, Args)]
pub struct CloneArgs {
    /// Source repository to clone labels from (OWNER/REPO).
    #[arg(value_name = "SOURCE_REPO")]
    source: String,

    /// Target repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Overwrite existing labels with the same name.
    #[arg(long)]
    force: bool,
}

impl CloneArgs {
    /// Run the label clone command.
    ///
    /// # Errors
    ///
    /// Returns an error if the labels cannot be cloned.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let source =
            Repo::from_full_name(&self.source).context("invalid source repository format")?;
        let target = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("target repository required (use -R OWNER/REPO)"))?;
        let target = Repo::from_full_name(target).context("invalid target repository format")?;

        let client = factory.api_client(source.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Fetch labels from source
        let source_path = format!(
            "repos/{}/{}/labels?per_page=100",
            source.owner(),
            source.name(),
        );
        let labels: Vec<Value> = client
            .rest(reqwest::Method::GET, &source_path, None)
            .await
            .context("failed to list labels from source repository")?;

        if labels.is_empty() {
            ios_eprintln!(ios, "No labels found in {}", source.full_name());
            return Ok(());
        }

        let target_client = factory.api_client(target.host())?;

        for label in &labels {
            let name = label.get("name").and_then(Value::as_str).unwrap_or("");
            let color = label.get("color").and_then(Value::as_str).unwrap_or("");
            let description = label
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");

            let body = serde_json::json!({
                "name": name,
                "color": color,
                "description": description,
            });

            let create_path = format!("repos/{}/{}/labels", target.owner(), target.name(),);

            match target_client
                .rest::<Value>(reqwest::Method::POST, &create_path, Some(&body))
                .await
            {
                Ok(_) => {
                    ios_eprintln!(ios, "{} Cloned label {}", cs.success_icon(), cs.bold(name));
                }
                Err(e) => {
                    if self.force {
                        // Try to update existing label
                        let encoded = ghc_core::text::percent_encode(name);
                        let update_path = format!(
                            "repos/{}/{}/labels/{encoded}",
                            target.owner(),
                            target.name(),
                        );
                        match target_client
                            .rest::<Value>(reqwest::Method::PATCH, &update_path, Some(&body))
                            .await
                        {
                            Ok(_) => {
                                ios_eprintln!(
                                    ios,
                                    "{} Updated label {}",
                                    cs.warning_icon(),
                                    cs.bold(name),
                                );
                            }
                            Err(update_err) => {
                                ios_eprintln!(
                                    ios,
                                    "{} Failed to clone label {}: {update_err}",
                                    cs.error_icon(),
                                    name,
                                );
                            }
                        }
                    } else {
                        ios_eprintln!(
                            ios,
                            "{} Skipped label {} (already exists): {e}",
                            cs.warning_icon(),
                            name,
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_post};

    #[tokio::test]
    async fn test_should_clone_labels() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/source/repo/labels",
            serde_json::json!([
                {"name": "bug", "color": "d73a4a", "description": "Something isn't working"},
                {"name": "enhancement", "color": "a2eeef", "description": "New feature"}
            ]),
        )
        .await;
        mock_rest_post(
            &h.server,
            "/repos/target/repo/labels",
            201,
            serde_json::json!({"name": "bug"}),
        )
        .await;

        let args = CloneArgs {
            source: "source/repo".into(),
            repo: Some("target/repo".into()),
            force: false,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Cloned label"));
    }

    #[tokio::test]
    async fn test_should_fail_without_target_repo() {
        let h = TestHarness::new().await;

        let args = CloneArgs {
            source: "source/repo".into(),
            repo: None,
            force: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
