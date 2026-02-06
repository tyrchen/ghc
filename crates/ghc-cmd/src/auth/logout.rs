//! `ghc auth logout` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/logout/logout.go`.

use clap::Args;

use ghc_api::http;
use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Log out of a GitHub account.
///
/// Removes stored authentication configuration for an account.
/// This does not revoke authentication tokens on the server.
///
/// If the logged-out account was the active account and other accounts
/// remain for the same host, the active account is automatically switched
/// and a notification is displayed.
#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// The hostname of the GitHub instance to log out of.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// The account to log out of.
    #[arg(short, long = "user")]
    user: Option<String>,
}

struct HostUser {
    host: String,
    user: String,
}

impl LogoutArgs {
    /// Run the logout command.
    ///
    /// # Errors
    ///
    /// Returns an error if the logout operation fails.
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;

        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let known_hosts = cfg.hosts();
        if known_hosts.is_empty() {
            anyhow::bail!("not logged in to any hosts");
        }

        // Validate hostname if provided
        if let Some(ref h) = self.hostname
            && !known_hosts.contains(h)
        {
            anyhow::bail!("not logged in to {h}");
        }

        // Validate user if both hostname and user provided
        if let Some(ref h) = self.hostname
            && let Some(ref u) = self.user
        {
            let known_users = cfg.authentication().users_for_host(h);
            if !known_users.contains(u) {
                anyhow::bail!("not logged in to {h} account {u}");
            }
        }

        let mut candidates = Vec::new();
        for host in &known_hosts {
            if let Some(ref h) = self.hostname
                && host != h
            {
                continue;
            }
            let known_users = cfg.authentication().users_for_host(host);
            for user in known_users {
                if let Some(ref u) = self.user
                    && &user != u
                {
                    continue;
                }
                candidates.push(HostUser {
                    host: host.clone(),
                    user,
                });
            }
        }

        let (hostname, username) = if candidates.is_empty() {
            anyhow::bail!("no accounts matched that criteria");
        } else if candidates.len() == 1 {
            (candidates[0].host.clone(), candidates[0].user.clone())
        } else if !ios.can_prompt() {
            anyhow::bail!(
                "unable to determine which account to log out of, please specify `--hostname` and `--user`"
            );
        } else {
            let prompts: Vec<String> = candidates
                .iter()
                .map(|c| format!("{} ({})", c.user, c.host))
                .collect();
            let prompter = factory.prompter();
            let selected =
                prompter.select("What account do you want to log out of?", None, &prompts)?;
            (
                candidates[selected].host.clone(),
                candidates[selected].user.clone(),
            )
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
                    "To erase credentials stored in GitHub CLI, first clear the value from the environment."
                );
                anyhow::bail!("");
            }
        }

        // Record pre-logout active user for auto-switch detection
        let pre_logout_active = cfg.authentication().active_user(&hostname);

        cfg.authentication_mut().logout(&hostname, &username)?;

        // Check if a new user was automatically activated
        let post_logout_active = cfg.authentication().active_user(&hostname);
        let has_switched = pre_logout_active.as_deref() != post_logout_active.as_deref()
            && post_logout_active.is_some();

        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Logged out of {hostname} account {}",
            cs.success_icon(),
            cs.bold(&username),
        );

        if let (true, Some(new_user)) = (has_switched, post_logout_active) {
            ios_eprintln!(
                ios,
                "{} Switched active account for {hostname} to {}",
                cs.success_icon(),
                cs.bold(&new_user),
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::{AuthConfig, MemoryConfig};

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_logout_single_user() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: Some("github.com".to_string()),
            user: Some("testuser".to_string()),
        };
        args.run(&h.factory).await.unwrap();
        assert!(
            h.stderr()
                .contains("Logged out of github.com account testuser")
        );
    }

    #[tokio::test]
    async fn test_should_error_when_not_logged_in() {
        let config = MemoryConfig::new();
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: None,
            user: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged in"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: Some("unknown.host".to_string()),
            user: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged in to"));
    }

    #[tokio::test]
    async fn test_should_logout_auto_selects_single_candidate() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: None,
            user: None,
        };
        args.run(&h.factory).await.unwrap();
        assert!(
            h.stderr()
                .contains("Logged out of github.com account testuser")
        );
    }

    #[tokio::test]
    async fn test_should_auto_switch_on_logout_of_active_user() {
        let mut config = MemoryConfig::new();
        config
            .login("github.com", "user1", "token1", "https")
            .unwrap();
        config
            .login("github.com", "user2", "token2", "https")
            .unwrap();
        // user2 is active since it was logged in last
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: Some("github.com".to_string()),
            user: Some("user2".to_string()),
        };
        args.run(&h.factory).await.unwrap();

        let stderr = h.stderr();
        assert!(stderr.contains("Logged out of github.com account user2"));
        assert!(stderr.contains("Switched active account for github.com to user1"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_user_on_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = LogoutArgs {
            hostname: Some("github.com".to_string()),
            user: Some("ghost".to_string()),
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not logged in to github.com account ghost")
        );
    }
}
