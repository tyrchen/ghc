//! `ghc gpg-key add` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Add a GPG key to your GitHub account.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Path to the GPG public key file, or `-` for stdin.
    #[arg(value_name = "KEY_FILE")]
    key_file: String,

    /// A descriptive title for the key.
    #[arg(short, long)]
    title: Option<String>,
}

impl AddArgs {
    /// Run the gpg-key add command.
    ///
    /// # Errors
    ///
    /// Returns an error if the key cannot be added.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let key_content = if self.key_file == "-" {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .context("failed to read from stdin")?;
            buf
        } else {
            std::fs::read_to_string(&self.key_file)
                .with_context(|| format!("failed to read key file: {}", self.key_file))?
        };

        let mut body = serde_json::json!({
            "armored_public_key": key_content.trim(),
        });

        if let Some(ref title) = self.title {
            body["name"] = Value::String(title.clone());
        }

        let result: Value = client
            .rest(reqwest::Method::POST, "user/gpg_keys", Some(&body))
            .await
            .context("failed to add GPG key")?;

        let id = result.get("id").and_then(Value::as_u64).unwrap_or(0);
        let key_id = result.get("key_id").and_then(Value::as_str).unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Added GPG key (ID: {id}, Key ID: {key_id})",
            cs.success_icon(),
        );

        Ok(())
    }
}
