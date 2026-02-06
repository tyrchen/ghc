//! `ghc label delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a label.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Label name to delete.
    #[arg(value_name = "NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl DeleteArgs {
    /// Run the label delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the label cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let encoded = ghc_core::text::percent_encode(&self.name);
        let path = format!("repos/{}/{}/labels/{encoded}", repo.owner(), repo.name(),);

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete label")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted label {} from {}",
            cs.success_icon(),
            cs.bold(&self.name),
            cs.bold(&repo.full_name()),
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete};

    #[tokio::test]
    async fn test_should_delete_label() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/repos/owner/repo/labels/bug", 204).await;

        let args = DeleteArgs {
            name: "bug".into(),
            repo: Some("owner/repo".into()),
            yes: true,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted label"));
        assert!(err.contains("bug"));
    }
}
