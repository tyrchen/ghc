//! `ghc gpg-key list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List GPG keys on your GitHub account.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the gpg-key list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the keys cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let keys: Vec<Value> = client
            .rest(reqwest::Method::GET, "user/gpg_keys", None)
            .await
            .context("failed to list GPG keys")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&keys)?);
            return Ok(());
        }

        if keys.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No GPG keys found on your account");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for key in &keys {
            let id = key.get("id").and_then(Value::as_u64).unwrap_or(0);
            let key_id = key.get("key_id").and_then(Value::as_str).unwrap_or("");
            let emails = key
                .get("emails")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|e| e.get("email").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let created_at = key.get("created_at").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                format!("{id}"),
                cs.bold(key_id),
                emails,
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
    async fn test_should_list_gpg_keys() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/user/gpg_keys",
            serde_json::json!([
                {
                    "id": 1,
                    "key_id": "3AA5C34371567BD2",
                    "emails": [{"email": "user@example.com"}],
                    "created_at": "2024-01-15T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs { json: vec![] };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("3AA5C34371567BD2"), "should contain key ID");
        assert!(stdout.contains("user@example.com"), "should contain email");
    }
}
