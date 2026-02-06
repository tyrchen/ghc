//! `ghc auth switch` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/switch/switch.go`.

use clap::Args;

use ghc_api::http;
use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Switch the active GitHub account for a host.
///
/// If the specified host has two accounts, the active account will be
/// switched automatically. If there are more than two accounts,
/// disambiguation will be required either through `--user` or an
/// interactive prompt.
#[derive(Debug, Args)]
pub struct SwitchArgs {
    /// The hostname of the GitHub instance to switch account for.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// The account to switch to.
    #[arg(short, long = "user")]
    user: Option<String>,
}

impl SwitchArgs {
    /// Run the switch command.
    ///
    /// # Errors
    ///
    /// Returns an error if the switch operation fails.
    pub fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let known_hosts = cfg.hosts();
        if known_hosts.is_empty() {
            anyhow::bail!("not logged in to any hosts");
        }

        if let Some(ref h) = self.hostname
            && !known_hosts.contains(h)
        {
            anyhow::bail!("not logged in to {h}");
        }

        let hostname = match &self.hostname {
            Some(h) => h.clone(),
            None => {
                if known_hosts.len() == 1 {
                    known_hosts[0].clone()
                } else if !ios.can_prompt() {
                    anyhow::bail!(
                        "unable to determine which host to switch account for, please specify `--hostname`"
                    );
                } else {
                    let prompter = factory.prompter();
                    let idx = prompter.select(
                        "What host do you want to switch account for?",
                        None,
                        &known_hosts,
                    )?;
                    known_hosts[idx].clone()
                }
            }
        };

        let username = if let Some(u) = &self.user {
            u.clone()
        } else {
            if !ios.can_prompt() {
                anyhow::bail!(
                    "unable to determine which account to switch to, please specify `--user`"
                );
            }
            let prompter = factory.prompter();
            let input = prompter.input("Which user do you want to switch to?", "")?;
            if input.is_empty() {
                anyhow::bail!("username cannot be empty");
            }
            input
        };

        // Check if token is writeable
        if let Some((_, source)) = cfg.authentication().active_token(&hostname) {
            let (_, writeable) = http::auth_token_writeable(&source);
            if !writeable {
                ios_eprintln!(
                    ios,
                    "The value of the {source} environment variable is being used for authentication."
                );
                ios_eprintln!(
                    ios,
                    "To have GitHub CLI manage credentials instead, first clear the value from the environment."
                );
                anyhow::bail!("");
            }
        }

        cfg.authentication_mut().switch_user(&hostname, &username)?;
        ios_eprintln!(ios, "Switched active account for {hostname} to {username}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::{AuthConfig, MemoryConfig};

    use crate::test_helpers::TestHarness;

    fn two_user_config() -> MemoryConfig {
        let mut config = MemoryConfig::new();
        config
            .login("github.com", "user1", "token1", "https")
            .unwrap();
        config
            .login("github.com", "user2", "token2", "https")
            .unwrap();
        config
    }

    #[tokio::test]
    async fn test_should_switch_user() {
        let config = two_user_config();
        let h = TestHarness::with_config(config).await;
        let args = SwitchArgs {
            hostname: Some("github.com".to_string()),
            user: Some("user1".to_string()),
        };
        args.run(&h.factory).unwrap();
        assert!(
            h.stderr()
                .contains("Switched active account for github.com to user1")
        );
    }

    #[tokio::test]
    async fn test_should_error_when_not_logged_in() {
        let config = MemoryConfig::new();
        let h = TestHarness::with_config(config).await;
        let args = SwitchArgs {
            hostname: None,
            user: None,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged in"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = SwitchArgs {
            hostname: Some("unknown.host".to_string()),
            user: Some("user1".to_string()),
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged in to"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_user() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = SwitchArgs {
            hostname: Some("github.com".to_string()),
            user: Some("ghost".to_string()),
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
    }
}
