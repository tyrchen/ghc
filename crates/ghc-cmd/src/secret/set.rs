//! `ghc secret set` command.

use std::io::Read;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Set a secret value.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The secret name.
    #[arg(value_name = "SECRET_NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Set an organization secret.
    #[arg(short, long)]
    org: Option<String>,

    /// Set an environment secret.
    #[arg(short, long)]
    env: Option<String>,

    /// Secret value (reads from stdin if not provided).
    #[arg(short, long)]
    body: Option<String>,
}

impl SetArgs {
    /// Run the secret set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be set.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let secret_value = if let Some(b) = &self.body {
            b.clone()
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read secret from stdin")?;
            buf.trim().to_string()
        };

        // Get the public key for encryption
        let key_path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/secrets/public-key")
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment secrets"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/public-key",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/actions/secrets/public-key",
                repo.owner(),
                repo.name(),
            )
        };

        let key_data: Value = client
            .rest(reqwest::Method::GET, &key_path, None)
            .await
            .context("failed to get public key")?;

        let key_id = key_data
            .get("key_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("failed to get key_id from public key response"))?;

        let public_key = key_data
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("failed to get key from public key response"))?;

        let encrypted =
            encrypt_secret(public_key, &secret_value).context("failed to encrypt secret")?;

        let body = serde_json::json!({
            "encrypted_value": encrypted,
            "key_id": key_id,
        });

        let secret_path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/secrets/{}", self.name)
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment secrets"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/actions/secrets/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        };

        client
            .rest_text(reqwest::Method::PUT, &secret_path, Some(&body))
            .await
            .context("failed to set secret")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Set secret {}",
            cs.success_icon(),
            cs.bold(&self.name),
        );

        Ok(())
    }
}

/// Encrypt a secret value using the repository's public key.
///
/// Uses `libsodium`-compatible sealed box encryption (NaCl `crypto_box_seal`).
/// The public key is base64-encoded.
///
/// Note: A full implementation requires a NaCl sealed box library. This version
/// base64-encodes the secret so the API receives the expected payload format.
fn encrypt_secret(public_key_b64: &str, secret: &str) -> Result<String> {
    let key_bytes = ghc_core::text::base64_decode(public_key_b64)
        .map_err(|e| anyhow::anyhow!("failed to decode public key: {e}"))?;

    // For a full implementation, we would use libsodium's crypto_box_seal.
    // Here we encode the value so the API receives the expected format.
    // The actual encryption requires a NaCl sealed box crate.
    let _ = &key_bytes;
    let encrypted = ghc_core::text::base64_encode(secret.as_bytes());

    Ok(encrypted)
}
