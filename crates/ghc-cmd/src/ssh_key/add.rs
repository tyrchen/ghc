//! `ghc ssh-key add` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Add an SSH key to your GitHub account.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Path to the public key file.
    #[arg(value_name = "KEY_FILE")]
    key_file: String,

    /// A descriptive title for the key.
    #[arg(short, long)]
    title: Option<String>,

    /// Key type.
    #[arg(long, value_parser = ["authentication", "signing"], default_value = "authentication")]
    key_type: String,
}

impl AddArgs {
    /// Run the ssh-key add command.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be added.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let key_content = std::fs::read_to_string(&self.key_file)
            .with_context(|| format!("failed to read key file: {}", self.key_file))?;

        let title = self.title.clone().unwrap_or_else(|| {
            // Use the comment part of the key or the filename
            key_content
                .split_whitespace()
                .nth(2)
                .unwrap_or(&self.key_file)
                .to_string()
        });

        let body = serde_json::json!({
            "title": title,
            "key": key_content.trim(),
        });

        let path = if self.key_type == "signing" {
            "user/ssh_signing_keys"
        } else {
            "user/keys"
        };

        let result: Value = client
            .rest(reqwest::Method::POST, path, Some(&body))
            .await
            .context("failed to add SSH key")?;

        let id = result.get("id").and_then(Value::as_u64).unwrap_or(0);

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Added SSH {} key (ID: {id}) with title {:?}",
            cs.success_icon(),
            self.key_type,
            title,
        );

        Ok(())
    }
}
