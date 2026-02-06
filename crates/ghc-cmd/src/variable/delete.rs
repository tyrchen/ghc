//! `ghc variable delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a variable.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The variable name to delete.
    #[arg(value_name = "VARIABLE_NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Delete an organization variable.
    #[arg(short, long)]
    org: Option<String>,

    /// Delete an environment variable.
    #[arg(short, long)]
    env: Option<String>,
}

impl DeleteArgs {
    /// Run the variable delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/variables/{}", self.name)
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment variables"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/actions/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        };

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete variable")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted variable {}",
            cs.success_icon(),
            cs.bold(&self.name),
        );

        Ok(())
    }
}
