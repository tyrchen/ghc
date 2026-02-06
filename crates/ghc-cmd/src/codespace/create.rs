//! `ghc codespace create` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Create a codespace.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Repository to create the codespace for (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Branch to create the codespace on.
    #[arg(short, long)]
    branch: Option<String>,

    /// Machine type for the codespace.
    #[arg(short, long)]
    machine: Option<String>,

    /// Display name for the codespace.
    #[arg(short, long)]
    display_name: Option<String>,

    /// Number of minutes after which the codespace will be auto-stopped.
    #[arg(long)]
    idle_timeout: Option<u32>,

    /// Maximum number of minutes the codespace can remain running.
    #[arg(long)]
    retention_period: Option<String>,

    /// Devcontainer path.
    #[arg(long)]
    devcontainer_path: Option<String>,

    /// Location preference.
    #[arg(short, long)]
    location: Option<String>,
}

impl CreateArgs {
    /// Run the codespace create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let repo_name = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo =
            ghc_core::repo::Repo::from_full_name(repo_name).context("invalid repository format")?;

        let mut body = serde_json::json!({
            "repository_id": 0,  // Will be resolved
        });

        // Resolve repository ID
        let repo_path = format!("repos/{}/{}", repo.owner(), repo.name(),);
        let repo_data: Value = client
            .rest(reqwest::Method::GET, &repo_path, None)
            .await
            .context("failed to look up repository")?;

        let repo_id = repo_data
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("could not determine repository ID"))?;

        body["repository_id"] = serde_json::json!(repo_id);

        if let Some(ref branch) = self.branch {
            body["ref"] = Value::String(branch.clone());
        }
        if let Some(ref machine) = self.machine {
            body["machine"] = Value::String(machine.clone());
        }
        if let Some(ref name) = self.display_name {
            body["display_name"] = Value::String(name.clone());
        }
        if let Some(timeout) = self.idle_timeout {
            body["idle_timeout_minutes"] = serde_json::json!(timeout);
        }
        if let Some(ref retention) = self.retention_period {
            body["retention_period_minutes"] = Value::String(retention.clone());
        }
        if let Some(ref devcontainer) = self.devcontainer_path {
            body["devcontainer_path"] = Value::String(devcontainer.clone());
        }
        if let Some(ref location) = self.location {
            body["location"] = Value::String(location.clone());
        }

        let result: Value = client
            .rest(reqwest::Method::POST, "user/codespaces", Some(&body))
            .await
            .context("failed to create codespace")?;

        let name = result.get("name").and_then(Value::as_str).unwrap_or("");
        let state = result.get("state").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Created codespace {} (state: {state})",
            cs.success_icon(),
            cs.bold(name),
        );

        Ok(())
    }
}
