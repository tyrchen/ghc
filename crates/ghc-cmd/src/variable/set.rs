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
    /// The variable name (not required when using --env-file).
    #[arg(value_name = "VARIABLE_NAME")]
    name: Option<String>,

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

    /// Path to a .env file for batch setting variables.
    #[arg(long, value_name = "FILE")]
    env_file: Option<String>,

    /// Visibility for organization variables.
    #[arg(long, value_parser = ["all", "private", "selected"])]
    visibility: Option<String>,
}

impl SetArgs {
    /// Run the variable set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable cannot be set.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        // Batch mode: read variables from .env file
        if let Some(ref env_file) = self.env_file {
            return self.run_batch(factory, env_file).await;
        }

        let name = self
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("variable name is required"))?;

        let var_value = if let Some(b) = &self.body {
            b.clone()
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read variable from stdin")?;
            buf.trim().to_string()
        };

        self.set_single_variable(factory, name, &var_value).await
    }

    /// Set a single variable.
    async fn set_single_variable(
        &self,
        factory: &crate::factory::Factory,
        name: &str,
        var_value: &str,
    ) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let mut body = serde_json::json!({
            "name": name,
            "value": var_value,
        });

        // Add visibility for org variables
        if self.org.is_some()
            && let Some(ref vis) = self.visibility
        {
            body["visibility"] = Value::String(vis.clone());
        }

        let (base_path, exists) = if let Some(ref org) = self.org {
            let check_path = format!("orgs/{org}/actions/variables/{name}");
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
                "repos/{}/{}/environments/{env}/variables/{name}",
                repo.owner(),
                repo.name(),
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
                "repos/{}/{}/actions/variables/{name}",
                repo.owner(),
                repo.name(),
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
            let update_path = format!("{base_path}/{name}");
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
            cs.bold(name),
        );

        Ok(())
    }

    /// Batch set variables from a .env file.
    async fn run_batch(&self, factory: &crate::factory::Factory, env_file: &str) -> Result<()> {
        let content = std::fs::read_to_string(env_file)
            .with_context(|| format!("failed to read env file: {env_file}"))?;

        let mut count = 0;
        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                if key.is_empty() {
                    continue;
                }

                self.set_single_variable(factory, key, value).await?;
                count += 1;
            }
        }

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Set {count} variable(s) from {env_file}",
            cs.success_icon(),
        );

        Ok(())
    }
}
