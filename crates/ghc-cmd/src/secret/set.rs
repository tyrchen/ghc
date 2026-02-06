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
    /// The secret name (not required when using --env-file).
    #[arg(value_name = "SECRET_NAME")]
    name: Option<String>,

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

    /// Path to a .env file for batch setting secrets.
    #[arg(long, value_name = "FILE")]
    env_file: Option<String>,

    /// Visibility for organization secrets.
    #[arg(long, value_parser = ["all", "private", "selected"])]
    visibility: Option<String>,
}

impl SetArgs {
    /// Run the secret set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be set.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        // Batch mode: read secrets from .env file
        if let Some(ref env_file) = self.env_file {
            return self.run_batch(factory, env_file).await;
        }

        let name = self
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("secret name is required"))?;

        let secret_value = if let Some(b) = &self.body {
            b.clone()
        } else {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read secret from stdin")?;
            buf.trim().to_string()
        };

        self.set_single_secret(factory, name, &secret_value).await
    }

    /// Set a single secret.
    async fn set_single_secret(
        &self,
        factory: &crate::factory::Factory,
        name: &str,
        secret_value: &str,
    ) -> Result<()> {
        let client = factory.api_client("github.com")?;

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
            encrypt_secret(public_key, secret_value).context("failed to encrypt secret")?;

        let mut body = serde_json::json!({
            "encrypted_value": encrypted,
            "key_id": key_id,
        });

        // Add visibility for org secrets
        if self.org.is_some()
            && let Some(ref vis) = self.visibility
        {
            body["visibility"] = Value::String(vis.clone());
        }

        let secret_path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/secrets/{name}")
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment secrets"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/{name}",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/actions/secrets/{name}",
                repo.owner(),
                repo.name(),
            )
        };

        client
            .rest_text(reqwest::Method::PUT, &secret_path, Some(&body))
            .await
            .context("failed to set secret")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Set secret {}", cs.success_icon(), cs.bold(name),);

        Ok(())
    }

    /// Batch set secrets from a .env file.
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

                self.set_single_secret(factory, key, value).await?;
                count += 1;
            }
        }

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Set {count} secret(s) from {env_file}",
            cs.success_icon(),
        );

        Ok(())
    }
}

/// Encrypt a secret value using the repository's public key.
///
/// Uses libsodium-compatible sealed box encryption (`crypto_box_seal`).
/// The public key is a base64-encoded Curve25519 public key obtained from
/// the GitHub API.
fn encrypt_secret(public_key_b64: &str, secret: &str) -> Result<String> {
    use crypto_box::aead::OsRng;

    let key_bytes = ghc_core::text::base64_decode(public_key_b64)
        .map_err(|e| anyhow::anyhow!("failed to decode public key: {e}"))?;

    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;

    let public_key = crypto_box::PublicKey::from(key_array);
    let encrypted = public_key
        .seal(&mut OsRng, secret.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to encrypt secret: {e}"))?;

    Ok(ghc_core::text::base64_encode(&encrypted))
}
