//! `ghc secret set` command.

use std::io::Read;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Set a secret value.
///
/// Set a value for a secret on one of the following levels:
/// - repository (default): available to GitHub Actions runs or Dependabot in a repository
/// - environment: available to GitHub Actions runs for a deployment environment in a repository
/// - organization: available to GitHub Actions runs, Dependabot, or Codespaces within an organization
/// - user: available to Codespaces for your user
///
/// Organization and user secrets can optionally be restricted to only be available to
/// specific repositories.
///
/// Secret values are locally encrypted before being sent to GitHub.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Set a secret for your user (Codespaces).
    #[arg(short, long)]
    user: bool,

    /// Secret value (reads from stdin if not provided).
    #[arg(short, long)]
    body: Option<String>,

    /// Path to a .env file for batch setting secrets.
    #[arg(short = 'f', long, value_name = "FILE")]
    env_file: Option<String>,

    /// Visibility for organization secrets.
    #[arg(long, value_parser = ["all", "private", "selected"])]
    visibility: Option<String>,

    /// List of repositories that can access an organization or user secret.
    #[arg(short, long, value_delimiter = ',')]
    repos: Vec<String>,

    /// No repositories can access the organization secret.
    #[arg(long)]
    no_repos_selected: bool,

    /// Print the encrypted, base64-encoded value instead of storing it on GitHub.
    #[arg(long)]
    no_store: bool,

    /// Set the application for a secret (actions, codespaces, or dependabot).
    #[arg(short, long, value_parser = ["actions", "codespaces", "dependabot"])]
    app: Option<String>,
}

impl SetArgs {
    /// Run the secret set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be set.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        self.validate()?;

        // Batch mode: read secrets from .env file
        if let Some(ref env_file) = self.env_file {
            return self.run_batch(factory, env_file).await;
        }

        let name = self
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("must pass name argument"))?;

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

    /// Validate flag combinations.
    fn validate(&self) -> Result<()> {
        let entity_count =
            u8::from(self.org.is_some()) + u8::from(self.env.is_some()) + u8::from(self.user);
        if entity_count > 1 {
            anyhow::bail!("specify only one of `--org`, `--env`, or `--user`");
        }
        if self.body.is_some() && self.env_file.is_some() {
            anyhow::bail!("specify only one of `--body` or `--env-file`");
        }
        if self.env_file.is_some() && self.no_store {
            anyhow::bail!("specify only one of `--env-file` or `--no-store`");
        }
        if !self.repos.is_empty() && self.no_repos_selected {
            anyhow::bail!("specify only one of `--repos` or `--no-repos-selected`");
        }
        if self.user && self.no_repos_selected {
            anyhow::bail!("`--no-repos-selected` must be omitted when used with `--user`");
        }
        Ok(())
    }

    /// Resolve the secret application (actions, codespaces, or dependabot).
    fn resolve_app(&self) -> &str {
        if let Some(ref app) = self.app {
            app.as_str()
        } else if self.user {
            "codespaces"
        } else {
            "actions"
        }
    }

    /// Resolve visibility, auto-setting to "selected" when --repos or --no-repos-selected is used.
    fn resolve_visibility(&self) -> Option<String> {
        if let Some(ref vis) = self.visibility {
            Some(vis.clone())
        } else if !self.repos.is_empty() || self.no_repos_selected {
            Some("selected".to_string())
        } else {
            None
        }
    }

    /// Set a single secret.
    #[allow(clippy::too_many_lines)]
    async fn set_single_secret(
        &self,
        factory: &crate::factory::Factory,
        name: &str,
        secret_value: &str,
    ) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let app = self.resolve_app();

        // Get the public key for encryption
        let key_path = if let Some(ref org) = self.org {
            format!("orgs/{org}/{app}/secrets/public-key")
        } else if self.user {
            "user/codespaces/secrets/public-key".to_string()
        } else if let Some(ref env) = self.env {
            let repo = self.resolve_repo()?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/public-key",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.resolve_repo()?;
            format!(
                "repos/{}/{}/{app}/secrets/public-key",
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

        // --no-store: print encrypted value and return
        if self.no_store {
            let ios = &factory.io;
            ghc_core::ios_println!(ios, "{encrypted}");
            return Ok(());
        }

        let mut body = serde_json::json!({
            "encrypted_value": encrypted,
            "key_id": key_id,
        });

        // Add visibility for org/user secrets
        if (self.org.is_some() || self.user)
            && let Some(vis) = self.resolve_visibility()
        {
            body["visibility"] = Value::String(vis);
        }

        // Resolve repository IDs for --repos
        if !self.repos.is_empty() {
            let repo_ids =
                resolve_repo_ids(&client, self.org.as_deref().unwrap_or(""), &self.repos).await?;
            body["selected_repository_ids"] = Value::Array(
                repo_ids
                    .into_iter()
                    .map(|id| Value::Number(serde_json::Number::from(id)))
                    .collect(),
            );
        }

        let secret_path = if let Some(ref org) = self.org {
            format!("orgs/{org}/{app}/secrets/{name}")
        } else if self.user {
            format!("user/codespaces/secrets/{name}")
        } else if let Some(ref env) = self.env {
            let repo = self.resolve_repo()?;
            format!(
                "repos/{}/{}/environments/{env}/secrets/{name}",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.resolve_repo()?;
            format!(
                "repos/{}/{}/{app}/secrets/{name}",
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

        let app_title = capitalize(app);
        let target = if self.user {
            "your user".to_string()
        } else if let Some(ref org) = self.org {
            org.clone()
        } else {
            self.repo
                .clone()
                .unwrap_or_else(|| "repository".to_string())
        };

        ios_eprintln!(
            ios,
            "{} Set {app_title} secret {} for {target}",
            cs.success_icon(),
            cs.bold(name),
        );

        Ok(())
    }

    /// Resolve repo from --repo flag.
    fn resolve_repo(&self) -> Result<Repo> {
        let name = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        Repo::from_full_name(name).context("invalid repository format")
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

/// Resolve repository names to IDs for org/user secrets with selected visibility.
async fn resolve_repo_ids(
    client: &ghc_api::client::Client,
    default_owner: &str,
    repo_names: &[String],
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(repo_names.len());
    for repo_name in repo_names {
        let full_name = if repo_name.contains('/') {
            repo_name.clone()
        } else if !default_owner.is_empty() {
            format!("{default_owner}/{repo_name}")
        } else {
            anyhow::bail!("repository name must be in OWNER/REPO format: {repo_name}");
        };
        let repo = Repo::from_full_name(&full_name)
            .with_context(|| format!("invalid repository name: {full_name}"))?;
        let path = format!("repos/{}/{}", repo.owner(), repo.name());
        let data: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .with_context(|| format!("failed to look up repository: {full_name}"))?;
        let id = data
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("failed to get ID for repository: {full_name}"))?;
        ids.push(id);
    }
    Ok(ids)
}

/// Capitalize the first letter of a string.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_encrypt_secret_with_valid_key() {
        // Generate a test keypair
        let secret_key = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
        let public_key = secret_key.public_key();
        let public_key_b64 = ghc_core::text::base64_encode(public_key.as_bytes());

        let encrypted = encrypt_secret(&public_key_b64, "my-secret-value").unwrap();

        // The result should be base64-encoded and non-empty
        assert!(!encrypted.is_empty());

        // Verify the encrypted value can be decoded back to bytes
        let encrypted_bytes =
            ghc_core::text::base64_decode(&encrypted).expect("should be valid base64");

        // Encrypted output = 32 (ephemeral pk) + 16 (tag) + plaintext length
        assert_eq!(encrypted_bytes.len(), 32 + 16 + "my-secret-value".len());
    }

    #[test]
    fn test_should_reject_invalid_public_key() {
        let result = encrypt_secret("invalid-base64!!!", "secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_should_reject_wrong_size_key() {
        // Valid base64 but wrong size (not 32 bytes)
        let short_key = ghc_core::text::base64_encode(&[0u8; 16]);
        let result = encrypt_secret(&short_key, "secret");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be 32 bytes"));
    }
}
