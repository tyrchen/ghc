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

    /// Delete a secret for your user (Codespaces).
    #[arg(short, long)]
    user: bool,

    /// Delete a secret for a specific application (actions, codespaces, or dependabot).
    #[arg(short, long, value_parser = ["actions", "codespaces", "dependabot"])]
    app: Option<String>,
}

impl DeleteArgs {
    /// Run the secret delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let entity_count =
            u8::from(self.org.is_some()) + u8::from(self.env.is_some()) + u8::from(self.user);
        if entity_count > 1 {
            anyhow::bail!("specify only one of `--org`, `--env`, or `--user`");
        }

        let client = factory.api_client("github.com")?;

        let app = if let Some(ref a) = self.app {
            a.as_str()
        } else if self.user {
            "codespaces"
        } else {
            "actions"
        };

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/{app}/secrets/{}", self.name)
        } else if self.user {
            format!("user/codespaces/secrets/{}", self.name)
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
                "repos/{}/{}/{app}/secrets/{}",
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

        let target = if self.user {
            "your user".to_string()
        } else if let Some(ref org) = self.org {
            org.clone()
        } else {
            self.repo
                .clone()
                .unwrap_or_else(|| "repository".to_string())
        };

        ios_eprintln!(
            ios,
            "{} Deleted secret {} from {target}",
            cs.success_icon(),
            cs.bold(&self.name),
        );

        Ok(())
    }
}
