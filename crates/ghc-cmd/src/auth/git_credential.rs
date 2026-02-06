//! `ghc auth git-credential` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/gitcredential/helper.go`.
//! Implements the git credential helper protocol so that git
//! can use ghc-stored tokens for HTTPS operations.

use std::io::BufRead;

use clap::Args;

use ghc_core::ios_println;

use crate::factory::Factory;

/// The username used for token-based authentication.
const TOKEN_USER: &str = "x-access-token";

/// Implements git credential helper protocol.
///
/// This command is called by git when it needs credentials.
/// It supports the `get`, `store`, and `erase` operations.
#[derive(Debug, Args)]
pub struct GitCredentialArgs {
    /// The git credential operation: get, store, or erase.
    operation: String,
}

impl GitCredentialArgs {
    /// Run the git-credential command.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        match self.operation.as_str() {
            "store" | "erase" => {
                // We pretend to implement these but do nothing
                Ok(())
            }
            "get" => self.handle_get(factory),
            other => anyhow::bail!("ghc auth git-credential: \"{other}\" operation not supported"),
        }
    }

    #[allow(clippy::unused_self)]
    fn handle_get(&self, factory: &Factory) -> anyhow::Result<()> {
        let mut wants = std::collections::HashMap::new();

        let stdin = std::io::stdin();
        for line_result in stdin.lock().lines() {
            let line = line_result?;
            if line.is_empty() {
                break;
            }
            if let Some((key, value)) = line.split_once('=') {
                if key == "url" {
                    if let Ok(u) = url::Url::parse(value) {
                        wants.insert("protocol".to_string(), u.scheme().to_string());
                        wants.insert("host".to_string(), u.host_str().unwrap_or("").to_string());
                        wants.insert("path".to_string(), u.path().to_string());
                        wants.insert("username".to_string(), u.username().to_string());
                        wants.insert(
                            "password".to_string(),
                            u.password().unwrap_or("").to_string(),
                        );
                    }
                } else {
                    wants.insert(key.to_string(), value.to_string());
                }
            }
        }

        // Only handle HTTPS
        if wants.get("protocol").is_none_or(|p| p != "https") {
            anyhow::bail!("");
        }

        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
        let auth = cfg.authentication();

        let lookup_host = wants.get("host").cloned().unwrap_or_default();
        let (mut token, source) = auth.active_token(&lookup_host).unzip();

        // Try stripping gist. prefix
        if token.is_none() && lookup_host.starts_with("gist.") {
            let stripped = lookup_host.strip_prefix("gist.").unwrap_or(&lookup_host);
            if let Some((t, _s)) = auth.active_token(stripped) {
                token = Some(t);
            }
        }

        let source = source.unwrap_or_default();

        let got_user = if source.ends_with("_TOKEN") {
            TOKEN_USER.to_string()
        } else {
            auth.active_user(&lookup_host)
                .unwrap_or_else(|| TOKEN_USER.to_string())
        };

        let got_token = token.unwrap_or_default();

        if got_user.is_empty() || got_token.is_empty() {
            anyhow::bail!("");
        }

        // Check username match
        let wants_username = wants.get("username").cloned().unwrap_or_default();
        if !wants_username.is_empty()
            && got_user != TOKEN_USER
            && !wants_username.eq_ignore_ascii_case(&got_user)
        {
            anyhow::bail!("");
        }

        let ios = &factory.io;
        let host = wants.get("host").cloned().unwrap_or_default();
        ios_println!(ios, "protocol=https");
        ios_println!(ios, "host={host}");
        ios_println!(ios, "username={got_user}");
        ios_println!(ios, "password={got_token}");

        Ok(())
    }
}
