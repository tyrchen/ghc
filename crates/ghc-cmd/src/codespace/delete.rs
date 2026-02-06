//! `ghc codespace delete` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Delete a codespace.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Name of the codespace to delete.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Delete all codespaces.
    #[arg(long)]
    all: bool,

    /// Skip confirmation prompt.
    #[arg(short, long)]
    force: bool,

    /// Filter by repository when using --all.
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of days since last used.
    #[arg(long)]
    days: Option<u32>,
}

impl DeleteArgs {
    /// Run the codespace delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be deleted.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if let Some(ref name) = self.codespace {
            let path = format!("user/codespaces/{name}");
            client
                .rest_text(reqwest::Method::DELETE, &path, None)
                .await
                .context("failed to delete codespace")?;

            ios_eprintln!(
                ios,
                "{} Deleted codespace {}",
                cs.success_icon(),
                cs.bold(name),
            );
        } else if self.all {
            let codespaces: serde_json::Value = client
                .rest(reqwest::Method::GET, "user/codespaces", None)
                .await
                .context("failed to list codespaces")?;

            let list = codespaces
                .get("codespaces")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

            for cs_item in list {
                let name = cs_item.get("name").and_then(Value::as_str).unwrap_or("");

                // Apply repo filter
                if let Some(ref repo_filter) = self.repo {
                    let full_name = cs_item
                        .pointer("/repository/full_name")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if full_name != repo_filter {
                        continue;
                    }
                }

                let path = format!("user/codespaces/{name}");
                client
                    .rest_text(reqwest::Method::DELETE, &path, None)
                    .await
                    .with_context(|| format!("failed to delete codespace {name}"))?;

                ios_eprintln!(
                    ios,
                    "{} Deleted codespace {}",
                    cs.success_icon(),
                    cs.bold(name),
                );
            }
        } else {
            anyhow::bail!("specify a codespace name with -c or use --all");
        }

        Ok(())
    }
}
