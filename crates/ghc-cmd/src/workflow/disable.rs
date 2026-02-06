//! `ghc workflow disable` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Disable a workflow.
#[derive(Debug, Args)]
pub struct DisableArgs {
    /// Workflow ID or filename.
    #[arg(value_name = "WORKFLOW")]
    workflow: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,
}

impl DisableArgs {
    /// Run the workflow disable command.
    ///
    /// # Errors
    ///
    /// Returns an error if the workflow cannot be disabled.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let path = format!(
            "repos/{}/{}/actions/workflows/{}/disable",
            repo.owner(),
            repo.name(),
            self.workflow,
        );

        client
            .rest_text(reqwest::Method::PUT, &path, None)
            .await
            .context("failed to disable workflow")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Disabled workflow {} in {}",
            cs.success_icon(),
            cs.bold(&self.workflow),
            cs.bold(&repo.full_name()),
        );

        Ok(())
    }
}
