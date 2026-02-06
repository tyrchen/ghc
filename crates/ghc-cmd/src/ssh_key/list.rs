//! `ghc ssh-key list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List SSH keys on your GitHub account.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ListArgs {
    /// Run the ssh-key list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the keys cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let keys: Vec<Value> = client
            .rest(reqwest::Method::GET, "user/keys", None)
            .await
            .context("failed to list SSH keys")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let arr = Value::Array(keys.clone());
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if keys.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No SSH keys found on your account");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for key in &keys {
            let id = key.get("id").and_then(Value::as_u64).unwrap_or(0);
            let title = key.get("title").and_then(Value::as_str).unwrap_or("");
            let key_str = key.get("key").and_then(Value::as_str).unwrap_or("");
            let created_at = key.get("created_at").and_then(Value::as_str).unwrap_or("");

            // Show only first/last part of the key
            let key_preview = if key_str.len() > 30 {
                format!("{}...{}", &key_str[..15], &key_str[key_str.len() - 10..])
            } else {
                key_str.to_string()
            };

            tp.add_row(vec![
                format!("{id}"),
                cs.bold(title),
                key_preview,
                created_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_ssh_keys() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user/keys",
            serde_json::json!([
                {
                    "id": 1,
                    "title": "Work laptop",
                    "key": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITest",
                    "created_at": "2024-01-15T10:00:00Z"
                },
                {
                    "id": 2,
                    "title": "Home desktop",
                    "key": "ssh-rsa AAAATest123",
                    "created_at": "2024-01-14T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("Work laptop"), "should contain key title");
        assert!(
            stdout.contains("Home desktop"),
            "should contain second key title"
        );
    }
}
