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

struct HostUser {
    host: String,
    user: String,
    active: bool,
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

        // Validate user if both hostname and user provided
        if let Some(ref h) = self.hostname
            && let Some(ref u) = self.user
        {
            let known_users = cfg.authentication().users_for_host(h);
            if !known_users.contains(u) {
                anyhow::bail!("not logged in to {h} account {u}");
            }
        }

        // Build candidates list
        let mut candidates = Vec::new();
        for host in &known_hosts {
            if let Some(ref h) = self.hostname
                && host != h
            {
                continue;
            }
            let active_user = cfg.authentication().active_user(host);
            let known_users = cfg.authentication().users_for_host(host);
            for user in known_users {
                if let Some(ref u) = self.user
                    && &user != u
                {
                    continue;
                }
                let is_active = active_user.as_deref() == Some(user.as_str());
                candidates.push(HostUser {
                    host: host.clone(),
                    user,
                    active: is_active,
                });
            }
        }

        let (hostname, username) = select_candidate(&candidates, ios.can_prompt(), factory)?;

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

        let cs = ios.color_scheme();

        if let Err(e) = cfg.authentication_mut().switch_user(&hostname, &username) {
            ios_eprintln!(
                ios,
                "{} Failed to switch account for {} to {}",
                cs.error_icon(),
                &hostname,
                cs.bold(&username),
            );
            return Err(e);
        }

        ios_eprintln!(
            ios,
            "{} Switched active account for {} to {}",
            cs.success_icon(),
            &hostname,
            cs.bold(&username),
        );

        Ok(())
    }
}

/// Select which candidate to switch to based on the available options.
fn select_candidate(
    candidates: &[HostUser],
    can_prompt: bool,
    factory: &Factory,
) -> anyhow::Result<(String, String)> {
    if candidates.is_empty() {
        anyhow::bail!("no accounts matched that criteria");
    }
    if candidates.len() == 1 {
        return Ok((candidates[0].host.clone(), candidates[0].user.clone()));
    }
    if candidates.len() == 2 && candidates[0].host == candidates[1].host {
        let host = candidates[0].host.clone();
        let user = if candidates[0].active {
            candidates[1].user.clone()
        } else {
            candidates[0].user.clone()
        };
        return Ok((host, user));
    }
    if !can_prompt {
        anyhow::bail!(
            "unable to determine which account to switch to, please specify `--hostname` and `--user`"
        );
    }
    let prompts: Vec<String> = candidates
        .iter()
        .map(|c| {
            let mut prompt = format!("{} ({})", c.user, c.host);
            if c.active {
                prompt += " - active";
            }
            prompt
        })
        .collect();
    let prompter = factory.prompter();
    let selected = prompter.select("What account do you want to switch to?", None, &prompts)?;
    Ok((
        candidates[selected].host.clone(),
        candidates[selected].user.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::{AuthConfig, MemoryConfig};

    use crate::test_helpers::TestHarness;

    fn two_user_config() -> MemoryConfig {
        let mut config = MemoryConfig::new();
        config
            .login("github.com", "user1", "token1", "https", false)
            .unwrap();
        config
            .login("github.com", "user2", "token2", "https", false)
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
    async fn test_should_auto_switch_for_two_accounts() {
        let config = two_user_config();
        // user2 is active (logged in last)
        let h = TestHarness::with_config(config).await;
        let args = SwitchArgs {
            hostname: Some("github.com".to_string()),
            user: None,
        };
        args.run(&h.factory).unwrap();
        // Should auto-switch to user1 (the inactive one)
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not logged in to github.com account ghost")
        );
    }
}
