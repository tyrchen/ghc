//! `ghc secret delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Delete a secret.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The secret name to delete.
    #[arg(value_name = "SECRET_NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Delete an organization secret.
    #[arg(short, long)]
    org: Option<String>,

    /// Delete an environment secret.
    #[arg(short, long)]
    env: Option<String>,
}

impl DeleteArgs {
    /// Run the secret delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/secrets/{}", self.name)
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment secrets"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/{}",
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
                "repos/{}/{}/actions/secrets/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        };

        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete secret")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Deleted secret {}",
            cs.success_icon(),
            cs.bold(&self.name),
        );

        Ok(())
    }
}
