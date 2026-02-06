//! `ghc gist delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Delete a gist.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The gist ID or URL to delete.
    #[arg(value_name = "GIST")]
    gist: String,
}

impl DeleteArgs {
    /// Run the gist delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the gist cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let gist_id = self.gist.rsplit('/').next().unwrap_or(&self.gist);

        let client = factory.api_client("github.com")?;
        let path = format!("gists/{gist_id}");

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete gist")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Deleted gist {gist_id}", cs.success_icon());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete};

    #[tokio::test]
    async fn test_should_delete_gist() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/gists/abc123", 204).await;

        let args = DeleteArgs {
            gist: "abc123".into(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted gist"));
        assert!(err.contains("abc123"));
    }

    #[tokio::test]
    async fn test_should_extract_gist_id_from_url() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/gists/xyz789", 204).await;

        let args = DeleteArgs {
            gist: "https://gist.github.com/xyz789".into(),
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Deleted gist"));
        assert!(err.contains("xyz789"));
    }
}
