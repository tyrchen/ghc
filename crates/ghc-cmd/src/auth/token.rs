//! `ghc auth token` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/token/token.go`.

use std::fmt::Write;

use clap::Args;

use ghc_core::instance;
use ghc_core::ios_println;

use crate::factory::Factory;

/// Print the authentication token for a hostname and account.
///
/// Without `--hostname`, the default host is chosen.
/// Without `--user`, the active account for the host is chosen.
#[derive(Debug, Args)]
pub struct TokenArgs {
    /// The hostname of the GitHub instance.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// The account to output the token for.
    #[arg(short, long = "user")]
    user: Option<String>,

    /// Search only secure credential store for authentication token.
    #[arg(long, hide = true)]
    secure_storage: bool,
}

impl TokenArgs {
    /// Run the token command.
    ///
    /// # Errors
    ///
    /// Returns an error if no token is found.
    pub fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let hostname = if let Some(h) = &self.hostname {
            instance::normalize_hostname(h)
        } else {
            cfg.authentication()
                .default_host()
                .unwrap_or_else(|| instance::GITHUB_COM.to_string())
        };

        // Look up token based on --user flag
        let token = if let Some(ref user) = self.user {
            cfg.authentication()
                .token_for_user(&hostname, user)
                .map(|(t, _)| t)
        } else {
            cfg.authentication().active_token(&hostname).map(|(t, _)| t)
        };

        match token {
            Some(val) if !val.is_empty() => {
                ios_println!(ios, "{val}");
                Ok(())
            }
            _ => {
                let mut msg = format!("no oauth token found for {hostname}");
                if let Some(ref user) = self.user {
                    let _ = write!(msg, " account {user}");
                }
                anyhow::bail!("{msg}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::{AuthConfig, MemoryConfig};

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_print_token_for_default_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_secret123");
        let h = TestHarness::with_config(config).await;
        let args = TokenArgs {
            hostname: None,
            user: None,
            secure_storage: false,
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "ghp_secret123");
    }

    #[tokio::test]
    async fn test_should_print_token_for_specified_host() {
        let config = MemoryConfig::new()
            .with_host("github.com", "user1", "ghp_token1")
            .with_host("ghe.corp.com", "user2", "ghp_token2");
        let h = TestHarness::with_config(config).await;
        let args = TokenArgs {
            hostname: Some("ghe.corp.com".to_string()),
            user: None,
            secure_storage: false,
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "ghp_token2");
    }

    #[tokio::test]
    async fn test_should_error_when_no_token_found() {
        let config = MemoryConfig::new();
        let h = TestHarness::with_config(config).await;
        let args = TokenArgs {
            hostname: None,
            user: None,
            secure_storage: false,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no oauth token"));
    }

    #[tokio::test]
    async fn test_should_print_token_for_specific_user() {
        let mut config = MemoryConfig::new();
        config
            .login("github.com", "user1", "ghp_token_user1", "https")
            .unwrap();
        config
            .login("github.com", "user2", "ghp_token_user2", "https")
            .unwrap();
        let h = TestHarness::with_config(config).await;

        let args = TokenArgs {
            hostname: Some("github.com".to_string()),
            user: Some("user1".to_string()),
            secure_storage: false,
        };
        args.run(&h.factory).unwrap();
        assert_eq!(h.stdout().trim(), "ghp_token_user1");
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_user() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_token1");
        let h = TestHarness::with_config(config).await;
        let args = TokenArgs {
            hostname: Some("github.com".to_string()),
            user: Some("ghost".to_string()),
            secure_storage: false,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no oauth token found for github.com account ghost")
        );
    }
}
