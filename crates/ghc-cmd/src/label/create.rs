//! `ghc label create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Create a label.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Label name.
    #[arg(value_name = "NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Label color (hex without #).
    #[arg(short, long)]
    color: Option<String>,

    /// Label description.
    #[arg(short, long)]
    description: Option<String>,

    /// Overwrite existing label with the same name.
    #[arg(short, long)]
    force: bool,
}

impl CreateArgs {
    /// Run the label create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the label cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let mut body = serde_json::json!({
            "name": self.name,
        });

        if let Some(ref color) = self.color {
            body["color"] = Value::String(color.trim_start_matches('#').to_string());
        }
        if let Some(ref desc) = self.description {
            body["description"] = Value::String(desc.clone());
        }

        let path = format!("repos/{}/{}/labels", repo.owner(), repo.name(),);

        let result = client
            .rest::<Value>(reqwest::Method::POST, &path, Some(&body))
            .await;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        match result {
            Ok(_) => {
                ios_eprintln!(
                    ios,
                    "{} Created label {} in {}",
                    cs.success_icon(),
                    cs.bold(&self.name),
                    cs.bold(&repo.full_name()),
                );
            }
            Err(e) if self.force => {
                // Try to update
                let encoded = ghc_core::text::percent_encode(&self.name);
                let update_path =
                    format!("repos/{}/{}/labels/{encoded}", repo.owner(), repo.name(),);
                client
                    .rest::<Value>(reqwest::Method::PATCH, &update_path, Some(&body))
                    .await
                    .context("failed to update existing label")?;
                ios_eprintln!(
                    ios,
                    "{} Updated label {} in {}",
                    cs.success_icon(),
                    cs.bold(&self.name),
                    cs.bold(&repo.full_name()),
                );
            }
            Err(e) => {
                return Err(e).context("failed to create label");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_label() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/labels",
            201,
            serde_json::json!({"name": "bug", "color": "d73a4a"}),
        )
        .await;

        let args = CreateArgs {
            name: "bug".into(),
            repo: Some("owner/repo".into()),
            color: Some("d73a4a".into()),
            description: Some("Something isn't working".into()),
            force: false,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created label"));
        assert!(err.contains("bug"));
    }

    #[tokio::test]
    async fn test_should_fail_without_repo_flag() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: "bug".into(),
            repo: None,
            color: None,
            description: None,
            force: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
