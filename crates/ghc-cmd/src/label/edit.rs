//! `ghc label edit` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Edit a label.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Label name to edit.
    #[arg(value_name = "NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// New label name.
    #[arg(long)]
    new_name: Option<String>,

    /// New label color (hex without #).
    #[arg(short, long)]
    color: Option<String>,

    /// New label description.
    #[arg(short, long)]
    description: Option<String>,
}

impl EditArgs {
    /// Run the label edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the label cannot be edited.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let mut body = serde_json::json!({});

        if let Some(ref new_name) = self.new_name {
            body["new_name"] = Value::String(new_name.clone());
        }
        if let Some(ref color) = self.color {
            body["color"] = Value::String(color.trim_start_matches('#').to_string());
        }
        if let Some(ref desc) = self.description {
            body["description"] = Value::String(desc.clone());
        }

        let encoded = ghc_core::text::percent_encode(&self.name);
        let path = format!("repos/{}/{}/labels/{encoded}", repo.owner(), repo.name(),);

        let _: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to edit label")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        let display_name = self.new_name.as_deref().unwrap_or(&self.name);
        ios_eprintln!(
            ios,
            "{} Edited label {} in {}",
            cs.success_icon(),
            cs.bold(display_name),
            cs.bold(&repo.full_name()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_edit_label() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo/labels/bug",
            200,
            serde_json::json!({"name": "critical-bug", "color": "ff0000"}),
        )
        .await;

        let args = EditArgs {
            name: "bug".into(),
            repo: Some("owner/repo".into()),
            new_name: Some("critical-bug".into()),
            color: Some("ff0000".into()),
            description: None,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Edited label"));
        assert!(err.contains("critical-bug"));
    }
}
