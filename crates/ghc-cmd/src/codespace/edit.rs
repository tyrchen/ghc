//! `ghc codespace edit` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Edit a codespace.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Name of the codespace to edit.
    #[arg(short, long)]
    codespace: Option<String>,

    /// New display name for the codespace.
    #[arg(short, long)]
    display_name: Option<String>,

    /// New machine type.
    #[arg(short, long)]
    machine: Option<String>,
}

impl EditArgs {
    /// Run the codespace edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be edited.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let client = factory.api_client("github.com")?;

        let mut body = serde_json::json!({});

        if let Some(ref display_name) = self.display_name {
            body["display_name"] = Value::String(display_name.clone());
        }
        if let Some(ref machine) = self.machine {
            body["machine"] = Value::String(machine.clone());
        }

        let path = format!("user/codespaces/{name}");
        let _: Value = client
            .rest(reqwest::Method::PATCH, &path, Some(&body))
            .await
            .context("failed to edit codespace")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Updated codespace {}",
            cs.success_icon(),
            cs.bold(name),
        );

        Ok(())
    }
}
