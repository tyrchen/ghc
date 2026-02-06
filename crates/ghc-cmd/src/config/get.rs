//! `ghc config get` command.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_println;

use crate::factory::Factory;

/// Print the value of a given configuration key.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The configuration key to read.
    key: String,
    /// Get per-host setting.
    #[arg(short = 'h', long)]
    host: Option<String>,
}

impl GetArgs {
    /// Run the config get command.
    ///
    /// # Errors
    ///
    /// Returns an error if the key is not found.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let hostname = self.host.as_deref().unwrap_or("");

        // Handle oauth_token specially
        if !hostname.is_empty() && self.key == "oauth_token" {
            let auth = cfg.authentication();
            if let Some((token, _)) = auth.active_token(hostname) {
                ios_println!(ios, "{token}");
                return Ok(());
            }
            anyhow::bail!("could not find key \"oauth_token\"");
        }

        match cfg.get(hostname, &self.key) {
            Some(val) if !val.is_empty() => {
                ios_println!(ios, "{val}");
                Ok(())
            }
            Some(_) => Ok(()), // empty value
            None => {
                // Check if it's a known key with a default
                let default = ghc_core::config::default_for_key(&self.key);
                if default.is_empty() {
                    anyhow::bail!("could not find key \"{}\"", self.key);
                }
                ios_println!(ios, "{default}");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::{Config, MemoryConfig};

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_get_default_git_protocol() {
        let h = TestHarness::new().await;
        let args = GetArgs {
            key: "git_protocol".to_string(),
            host: None,
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "https");
    }

    #[tokio::test]
    async fn test_should_get_set_value() {
        let mut config = MemoryConfig::new();
        config.set("", "editor", "vim").unwrap();
        let h = TestHarness::with_config(config).await;
        let args = GetArgs {
            key: "editor".to_string(),
            host: None,
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "vim");
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_key() {
        let h = TestHarness::new().await;
        let args = GetArgs {
            key: "nonexistent_key".to_string(),
            host: None,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("could not find key")
        );
    }

    #[tokio::test]
    async fn test_should_get_oauth_token_for_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_secret123");
        let h = TestHarness::with_config(config).await;
        let args = GetArgs {
            key: "oauth_token".to_string(),
            host: Some("github.com".to_string()),
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "ghp_secret123");
    }
}
