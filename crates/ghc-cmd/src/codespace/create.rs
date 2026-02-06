//! `ghc codespace create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

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

    /// Show status after creation.
    #[arg(short, long)]
    status: bool,

    /// Open in browser instead of creating via API.
    #[arg(short, long)]
    web: bool,
}

impl CreateArgs {
    /// Run the codespace create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo_name = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo =
            ghc_core::repo::Repo::from_full_name(repo_name).context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://github.com/codespaces/new?repo={}/{}",
                repo.owner(),
                repo.name()
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        // Resolve repository ID
        let repo_path = format!("repos/{}/{}", repo.owner(), repo.name());
        let repo_data: Value = client
            .rest(reqwest::Method::GET, &repo_path, None::<&Value>)
            .await
            .context("failed to look up repository")?;

        let repo_id = repo_data
            .get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("could not determine repository ID"))?;

        // Determine machine type (interactive selection if not provided)
        let machine_name = if let Some(ref m) = self.machine {
            m.clone()
        } else if ios.can_prompt() {
            self.select_machine(&client, &repo, repo_id, factory)
                .await?
        } else {
            String::new()
        };

        let mut body = serde_json::json!({
            "repository_id": repo_id,
        });

        if let Some(ref branch) = self.branch {
            body["ref"] = Value::String(branch.clone());
        }
        if !machine_name.is_empty() {
            body["machine"] = Value::String(machine_name);
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

        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Created codespace {} (state: {state})",
            cs.success_icon(),
            cs.bold(name),
        );

        if self.status {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&result)?);
        }

        Ok(())
    }

    /// Fetch available machine types and prompt the user to select one.
    async fn select_machine(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        repo_id: u64,
        factory: &crate::factory::Factory,
    ) -> Result<String> {
        let mut path = format!("repos/{}/{}/codespaces/machines", repo.owner(), repo.name());
        if let Some(ref branch) = self.branch {
            use std::fmt::Write;
            let _ = write!(path, "?ref={branch}");
        }

        let machines: Value = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .with_context(|| {
                format!(
                    "failed to fetch available machines for {}/{} (repo_id={repo_id})",
                    repo.owner(),
                    repo.name()
                )
            })?;

        let machine_list = machines
            .get("machines")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("no machine types available for this repository"))?;

        if machine_list.is_empty() {
            return Err(anyhow::anyhow!(
                "no machine types available for this repository"
            ));
        }

        let items: Vec<String> = machine_list
            .iter()
            .filter_map(|m: &Value| {
                let name = m.get("name")?.as_str()?;
                let display = m.get("display_name")?.as_str()?;
                let cpus = m.get("cpus").and_then(Value::as_u64).unwrap_or(0);
                let mem_bytes = m
                    .get("memory_in_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let mem_gb = mem_bytes / (1024 * 1024 * 1024);
                let storage_bytes = m
                    .get("storage_in_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let storage_gb = storage_bytes / (1024 * 1024 * 1024);
                Some(format!(
                    "{name}: {display} ({cpus} cores, {mem_gb}GB RAM, {storage_gb}GB disk)"
                ))
            })
            .collect();

        if items.is_empty() {
            return Err(anyhow::anyhow!(
                "no machine types available for this repository"
            ));
        }

        let prompter = factory.prompter();
        let selection = prompter
            .select("Choose a machine type", Some(0), &items)
            .context("failed to read machine type selection")?;

        // Extract machine name (before the first colon)
        let selected = &items[selection];
        let machine_name = selected
            .split(':')
            .next()
            .ok_or_else(|| anyhow::anyhow!("invalid machine selection"))?
            .to_string();

        Ok(machine_name)
    }
}
