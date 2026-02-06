//! `ghc auth status` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/status/status.go`.

use std::collections::BTreeMap;

use clap::Args;
use serde::Serialize;

use ghc_api::client;
use ghc_core::{ios_eprintln, ios_println};

use crate::factory::Factory;

/// Display active account and authentication state on each known GitHub host.
///
/// For each host, the authentication state is tested and any issues
/// are included in the output. Each host section will indicate the active
/// account, which will be used when targeting that host.
///
/// If an account on any host has authentication issues, the command exits
/// with code 1 and output goes to stderr. When using the `--json` option,
/// the command always exits with zero unless there is a fatal error.
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

    /// Output in JSON format. Supported fields: hosts.
    #[arg(long)]
    json: bool,
}

/// JSON output structure for auth status.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthStatusJson {
    hosts: BTreeMap<String, Vec<AuthEntryJson>>,
}

/// A single auth entry in the JSON output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthEntryJson {
    state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
    active: bool,
    host: String,
    login: String,
    token_source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    token: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    scopes: String,
    git_protocol: String,
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
            if self.json {
                let empty = AuthStatusJson {
                    hosts: BTreeMap::new(),
                };
                ios_println!(ios, "{}", serde_json::to_string_pretty(&empty)?);
                return Ok(());
            }
            anyhow::bail!("");
        }

        if let Some(ref h) = self.hostname
            && !hostnames.contains(h)
        {
            ios_eprintln!(ios, "You are not logged into any accounts on {h}");
            if self.json {
                let empty = AuthStatusJson {
                    hosts: BTreeMap::new(),
                };
                ios_println!(ios, "{}", serde_json::to_string_pretty(&empty)?);
                return Ok(());
            }
            anyhow::bail!("");
        }

        // Build status entries for all hosts/users
        let mut statuses: BTreeMap<String, Vec<AuthEntryJson>> = BTreeMap::new();
        let mut has_error = false;

        for hostname in &hostnames {
            if let Some(ref h) = self.hostname
                && hostname != h
            {
                continue;
            }

            let auth = cfg.authentication();
            let git_protocol = cfg.git_protocol(hostname);

            // Active user entry
            if let Some((token, source)) = auth.active_token(hostname) {
                let username = auth.active_user(hostname).unwrap_or_default();
                let display_username = if username.is_empty() {
                    "unknown".to_string()
                } else {
                    username.clone()
                };

                let mut entry = AuthEntryJson {
                    state: "success".to_string(),
                    error: String::new(),
                    active: true,
                    host: hostname.clone(),
                    login: display_username,
                    token_source: source.clone(),
                    token: token.clone(),
                    scopes: String::new(),
                    git_protocol: git_protocol.clone(),
                };

                if client::expect_scopes(&token) {
                    drop(cfg);
                    let api_client = factory.api_client(hostname)?;
                    match api_client.get_scopes(&token).await {
                        Ok(scopes) => {
                            entry.scopes.clone_from(&scopes);
                            if let Err(_e) = client::check_minimum_scopes(&scopes) {
                                has_error = true;
                            }
                        }
                        Err(e) => {
                            entry.state = "error".to_string();
                            entry.error = e.to_string();
                            has_error = true;
                        }
                    }
                    statuses.entry(hostname.clone()).or_default().push(entry);
                    // For now, only handle single host when scopes need API call
                    break;
                }

                statuses.entry(hostname.clone()).or_default().push(entry);

                // Non-active users (if not --active only)
                if !self.active {
                    let users = auth.users_for_host(hostname);
                    for user in &users {
                        if Some(user.as_str()) == auth.active_user(hostname).as_deref() {
                            continue;
                        }
                        if let Some((tok, tok_src)) = auth.token_for_user(hostname, user) {
                            let mut inactive_entry = AuthEntryJson {
                                state: "success".to_string(),
                                error: String::new(),
                                active: false,
                                host: hostname.clone(),
                                login: user.clone(),
                                token_source: tok_src,
                                token: tok,
                                scopes: String::new(),
                                git_protocol: git_protocol.clone(),
                            };
                            // We skip scope checking for inactive users in non-JSON mode
                            // to avoid multiple API calls
                            if self.json {
                                inactive_entry.state = "success".to_string();
                            }
                            statuses
                                .entry(hostname.clone())
                                .or_default()
                                .push(inactive_entry);
                        }
                    }
                }
            } else {
                statuses
                    .entry(hostname.clone())
                    .or_default()
                    .push(AuthEntryJson {
                        state: "error".to_string(),
                        error: format!("no token found for {hostname}"),
                        active: true,
                        host: hostname.clone(),
                        login: String::new(),
                        token_source: String::new(),
                        token: String::new(),
                        scopes: String::new(),
                        git_protocol: git_protocol.clone(),
                    });
                has_error = true;
            }
        }

        // Mask tokens unless --show-token
        if !self.show_token {
            for entries in statuses.values_mut() {
                for entry in entries.iter_mut() {
                    if self.json {
                        entry.token.clear();
                    } else {
                        entry.token = client::mask_token(&entry.token);
                    }
                }
            }
        }

        // JSON output
        if self.json {
            let output = AuthStatusJson { hosts: statuses };
            ios_println!(ios, "{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }

        // Human-readable output
        let cs = ios.color_scheme();
        let mut first_host = true;
        for (hostname, entries) in &statuses {
            if !first_host {
                ios_println!(ios, "");
            }
            first_host = false;
            ios_println!(ios, "{}", cs.bold(hostname));

            for entry in entries {
                match entry.state.as_str() {
                    "success" => {
                        ios_println!(
                            ios,
                            "  {} Logged in to {} account {} ({})",
                            cs.success_icon(),
                            entry.host,
                            cs.bold(&entry.login),
                            entry.token_source,
                        );
                        ios_println!(
                            ios,
                            "  - Active account: {}",
                            cs.bold(&entry.active.to_string()),
                        );
                        ios_println!(
                            ios,
                            "  - Git operations protocol: {}",
                            cs.bold(&entry.git_protocol),
                        );
                        ios_println!(ios, "  - Token: {}", cs.bold(&entry.token));

                        if !entry.scopes.is_empty() || client::expect_scopes(&entry.token) {
                            let display = display_scopes(&entry.scopes);
                            ios_println!(ios, "  - Token scopes: {}", cs.bold(&display));

                            if let Err(e) = client::check_minimum_scopes(&entry.scopes)
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
                                    cs.bold(&format!("ghc auth refresh -h {}", entry.host)),
                                );
                            }
                        }
                    }
                    "error" => {
                        if entry.login.is_empty() {
                            ios_println!(
                                ios,
                                "  {} Failed to log in to {} using token ({})",
                                cs.error_icon(),
                                entry.host,
                                entry.token_source,
                            );
                        } else {
                            ios_println!(
                                ios,
                                "  {} Failed to log in to {} account {} ({})",
                                cs.error_icon(),
                                entry.host,
                                cs.bold(&entry.login),
                                entry.token_source,
                            );
                        }
                        ios_println!(
                            ios,
                            "  - Active account: {}",
                            cs.bold(&entry.active.to_string()),
                        );
                        if !entry.error.is_empty() {
                            ios_println!(
                                ios,
                                "  - The token in {} is invalid.",
                                entry.token_source,
                            );
                        }
                    }
                    _ => {
                        ios_println!(
                            ios,
                            "  {} No token found for {}",
                            cs.error_icon(),
                            entry.host,
                        );
                    }
                }
            }
        }

        if has_error {
            anyhow::bail!("")
        }
        Ok(())
    }
}

fn display_scopes(scopes: &str) -> String {
    if scopes.is_empty() {
        return "none".to_string();
    }
    scopes
        .split(',')
        .map(|s| format!("'{}'", s.trim()))
        .collect::<Vec<_>>()
        .join(", ")
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
            json: false,
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
            json: false,
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
            json: false,
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
            json: false,
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
            json: false,
        };
        let result = args.run(&factory).await;
        assert!(result.is_err());
        assert!(
            output
                .stderr()
                .contains("not logged into any accounts on unknown.host")
        );
    }

    #[tokio::test]
    async fn test_should_output_json_format() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let h = TestHarness::with_config(config).await;

        let args = StatusArgs {
            hostname: None,
            show_token: false,
            active: false,
            json: true,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert!(parsed["hosts"]["github.com"].is_array());
        let entries = parsed["hosts"]["github.com"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["login"], "testuser");
        assert_eq!(entries[0]["state"], "success");
        assert_eq!(entries[0]["active"], true);
        // Token should be empty in JSON when --show-token not provided
        assert_eq!(entries[0]["token"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_should_output_json_with_token() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "github_pat_test123");
        let h = TestHarness::with_config(config).await;

        let args = StatusArgs {
            hostname: None,
            show_token: true,
            active: false,
            json: true,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        let entries = parsed["hosts"]["github.com"].as_array().unwrap();
        assert_eq!(entries[0]["token"], "github_pat_test123");
    }

    #[tokio::test]
    async fn test_should_return_empty_json_when_not_logged_in() {
        let config = MemoryConfig::new();
        let (factory, output) = Factory::test();
        let (factory, _browser) = factory.with_stub_browser();
        let (factory, _prompter) = factory.with_stub_prompter();
        let factory = factory.with_config(Box::new(config));

        let args = StatusArgs {
            hostname: None,
            show_token: false,
            active: false,
            json: true,
        };
        // JSON mode should not error
        args.run(&factory).await.unwrap();
        let stdout = output.stdout();
        let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert!(parsed["hosts"].as_object().unwrap().is_empty());
    }

    #[test]
    fn test_should_display_scopes() {
        assert_eq!(display_scopes(""), "none");
        assert_eq!(display_scopes("repo"), "'repo'");
        assert_eq!(
            display_scopes("repo, read:org, gist"),
            "'repo', 'read:org', 'gist'"
        );
    }
}
