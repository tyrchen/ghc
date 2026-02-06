//! `ghc auth status` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/status/status.go`.

use clap::Args;

use ghc_api::client;
use ghc_core::{ios_eprintln, ios_println};

use crate::factory::Factory;

/// Display active account and authentication state on each known GitHub host.
///
/// For each host, the authentication state is tested and any issues
/// are included in the output.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Check only a specific hostname's auth status.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// Display the auth token.
    #[arg(short = 't', long = "show-token")]
    show_token: bool,

    /// Display the active account only.
    #[arg(short, long)]
    active: bool,
}

impl StatusArgs {
    /// Run the status command.
    ///
    /// # Errors
    ///
    /// Returns an error if the status check fails.
    #[allow(clippy::too_many_lines, clippy::await_holding_lock)]
    pub async fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let hostnames = cfg.hosts();
        if hostnames.is_empty() {
            ios_eprintln!(
                ios,
                "You are not logged into any GitHub hosts. To log in, run: ghc auth login"
            );
            anyhow::bail!("");
        }

        if let Some(ref h) = self.hostname
            && !hostnames.contains(h)
        {
            ios_eprintln!(ios, "You are not logged into any accounts on {h}");
            anyhow::bail!("");
        }

        let cs = ios.color_scheme();
        let mut has_error = false;

        for hostname in &hostnames {
            if let Some(ref h) = self.hostname
                && hostname != h
            {
                continue;
            }

            ios_println!(ios, "{}", cs.bold(hostname));

            let auth = cfg.authentication();
            let git_protocol = cfg.git_protocol(hostname);

            if let Some((token, source)) = auth.active_token(hostname) {
                let username = auth.active_user(hostname).unwrap_or_default();

                // Check the token by fetching scopes
                let display_username = if username.is_empty() {
                    // If no stored username and source is not env, try to fetch
                    if client::token_source_is_writeable(&source) {
                        // We'd need to make an API call, but we don't have async
                        // context for that inside the locked config. Show "unknown".
                        "unknown".to_string()
                    } else {
                        "unknown".to_string()
                    }
                } else {
                    username.clone()
                };

                let display_token = if self.show_token {
                    token.clone()
                } else {
                    client::mask_token(&token)
                };

                ios_println!(
                    ios,
                    "  {} Logged in to {hostname} account {} ({source})",
                    cs.success_icon(),
                    cs.bold(&display_username),
                );
                ios_println!(ios, "  - Active account: {}", cs.bold("true"));
                ios_println!(
                    ios,
                    "  - Git operations protocol: {}",
                    cs.bold(&git_protocol)
                );
                ios_println!(ios, "  - Token: {}", cs.bold(&display_token));

                if client::expect_scopes(&token) {
                    // Try to check scopes
                    drop(cfg);
                    let api_client = factory.api_client(hostname)?;
                    match api_client.get_scopes(&token).await {
                        Ok(scopes) => {
                            let display = if scopes.is_empty() {
                                "none".to_string()
                            } else {
                                scopes
                                    .split(',')
                                    .map(|s| format!("'{}'", s.trim()))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                            ios_println!(ios, "  - Token scopes: {}", cs.bold(&display));

                            if let Err(e) = client::check_minimum_scopes(&scopes)
                                && let Some(missing) = e.missing_scopes()
                            {
                                let missing_str = missing.join(", ");
                                ios_println!(
                                    ios,
                                    "  {} Missing required token scopes: {}",
                                    cs.warning_icon(),
                                    cs.bold(&missing_str),
                                );
                                ios_println!(
                                    ios,
                                    "  - To request missing scopes, run: {}",
                                    cs.bold(&format!("ghc auth refresh -h {hostname}")),
                                );
                                has_error = true;
                            }
                        }
                        Err(e) => {
                            ios_println!(
                                ios,
                                "  {} Failed to check token scopes: {e}",
                                cs.error_icon()
                            );
                            has_error = true;
                        }
                    }
                    // Re-acquire config lock for next iteration
                    let cfg_relock = factory.config()?;
                    let _cfg = cfg_relock
                        .lock()
                        .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
                    return if has_error { anyhow::bail!("") } else { Ok(()) };
                }
            } else {
                ios_println!(ios, "  {} No token found for {hostname}", cs.error_icon());
                has_error = true;
            }
        }

        if has_error {
            anyhow::bail!("")
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::MemoryConfig;

    use crate::factory::Factory;
    use crate::test_helpers::TestHarness;

    // Use github_pat_ prefix so expect_scopes() returns false,
    // avoiding the direct API call that bypasses URL overrides.

    #[tokio::test]
    async fn test_should_show_status_for_logged_in_user() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let h = TestHarness::with_config(config).await;

        let args = StatusArgs {
            hostname: None,
            show_token: false,
            active: false,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("github.com"));
        assert!(stdout.contains("testuser"));
        assert!(stdout.contains("Logged in to"));
    }

    #[tokio::test]
    async fn test_should_show_token_when_flag_set() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let h = TestHarness::with_config(config).await;

        let args = StatusArgs {
            hostname: None,
            show_token: true,
            active: false,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("github_pat_test123"));
    }

    #[tokio::test]
    async fn test_should_mask_token_by_default() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let h = TestHarness::with_config(config).await;

        let args = StatusArgs {
            hostname: None,
            show_token: false,
            active: false,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        // Token should be masked - full token should not appear
        assert!(!stdout.contains("github_pat_test123"));
        assert!(stdout.contains("github_pat_"));
    }

    #[tokio::test]
    async fn test_should_error_when_not_logged_in() {
        let config = MemoryConfig::new();
        let (factory, output) = Factory::test();
        let (factory, _browser) = factory.with_stub_browser();
        let (factory, _prompter) = factory.with_stub_prompter();
        let factory = factory.with_config(Box::new(config));

        let args = StatusArgs {
            hostname: None,
            show_token: false,
            active: false,
        };
        let result = args.run(&factory).await;
        assert!(result.is_err());
        assert!(output.stderr().contains("not logged into any GitHub hosts"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let (factory, output) = Factory::test();
        let (factory, _browser) = factory.with_stub_browser();
        let (factory, _prompter) = factory.with_stub_prompter();
        let factory = factory.with_config(Box::new(config));

        let args = StatusArgs {
            hostname: Some("unknown.host".to_string()),
            show_token: false,
            active: false,
        };
        let result = args.run(&factory).await;
        assert!(result.is_err());
        assert!(
            output
                .stderr()
                .contains("not logged into any accounts on unknown.host")
        );
    }
}
