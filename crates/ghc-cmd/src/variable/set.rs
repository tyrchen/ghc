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
    #[arg(short = 'f', long, value_name = "FILE")]
    env_file: Option<String>,

    /// Visibility for organization variables.
    #[arg(long, value_parser = ["all", "private", "selected"])]
    visibility: Option<String>,

    /// List of repositories that can access an organization variable.
    #[arg(short, long, value_delimiter = ',')]
    repos: Vec<String>,
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
        if self.org.is_some() {
            if let Some(ref vis) = self.visibility {
                body["visibility"] = Value::String(vis.clone());
            } else if !self.repos.is_empty() {
                body["visibility"] = Value::String("selected".to_string());
            }
        }

        // Resolve repository IDs for --repos
        if !self.repos.is_empty() && self.org.is_some() {
            let repo_ids =
                resolve_repo_ids(&client, self.org.as_deref().unwrap_or(""), &self.repos).await?;
            body["selected_repository_ids"] = Value::Array(
                repo_ids
                    .into_iter()
                    .map(|id| Value::Number(serde_json::Number::from(id)))
                    .collect(),
            );
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

/// Resolve repository names to IDs for org variables with selected visibility.
async fn resolve_repo_ids(
    client: &ghc_api::client::Client,
    default_owner: &str,
    repo_names: &[String],
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(repo_names.len());
    for repo_name in repo_names {
        let full_name = if repo_name.contains('/') {
            repo_name.clone()
        } else if !default_owner.is_empty() {
            format!("{default_owner}/{repo_name}")
        } else {
            anyhow::bail!("repository name must be in OWNER/REPO format: {repo_name}");
        };
        let repo = Repo::from_full_name(&full_name)
            .with_context(|| format!("invalid repository name: {full_name}"))?;
        let path = format!("repos/{}/{}", repo.owner(), repo.name());
        let data: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .with_context(|| format!("failed to look up repository: {full_name}"))?;
        let id = data
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("failed to get ID for repository: {full_name}"))?;
        ids.push(id);
    }
    Ok(ids)
}
