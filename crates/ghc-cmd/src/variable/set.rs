//! `ghc variable set` command.

use std::io::Read;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Set a variable value.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The variable name.
    #[arg(value_name = "VARIABLE_NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Set an organization variable.
    #[arg(short, long)]
    org: Option<String>,

    /// Set an environment variable.
    #[arg(short, long)]
    env: Option<String>,

    /// Variable value (reads from stdin if not provided).
    #[arg(short, long)]
    body: Option<String>,
}

impl SetArgs {
    /// Run the variable set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable cannot be set.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let var_value = if let Some(b) = &self.body {
            b.clone()
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read variable from stdin")?;
            buf.trim().to_string()
        };

        let body = serde_json::json!({
            "name": self.name,
            "value": var_value,
        });

        let (base_path, exists) = if let Some(ref org) = self.org {
            let check_path = format!("orgs/{org}/actions/variables/{}", self.name);
            let exists = client
                .rest::<Value>(reqwest::Method::GET, &check_path, None)
                .await
                .is_ok();
            (format!("orgs/{org}/actions/variables"), exists)
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment variables"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            let check_path = format!(
                "repos/{}/{}/environments/{env}/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            );
            let exists = client
                .rest::<Value>(reqwest::Method::GET, &check_path, None)
                .await
                .is_ok();
            (
                format!(
                    "repos/{}/{}/environments/{env}/variables",
                    repo.owner(),
                    repo.name(),
                ),
                exists,
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            let check_path = format!(
                "repos/{}/{}/actions/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            );
            let exists = client
                .rest::<Value>(reqwest::Method::GET, &check_path, None)
                .await
                .is_ok();
            (
                format!("repos/{}/{}/actions/variables", repo.owner(), repo.name(),),
                exists,
            )
        };

        if exists {
            // Update existing variable
            let update_path = format!("{base_path}/{}", self.name);
            client
                .rest_text(reqwest::Method::PATCH, &update_path, Some(&body))
                .await
                .context("failed to update variable")?;
        } else {
            // Create new variable
            client
                .rest_text(reqwest::Method::POST, &base_path, Some(&body))
                .await
                .context("failed to create variable")?;
        }

        let ios = &factory.io;
        let cs = ios.color_scheme();
        let action = if exists { "Updated" } else { "Set" };
        ios_eprintln!(
            ios,
            "{} {action} variable {}",
            cs.success_icon(),
            cs.bold(&self.name),
        );

        Ok(())
    }
}
