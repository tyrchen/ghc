//! In-memory configuration for testing.
//!
//! Provides a [`MemoryConfig`] that implements [`Config`] and [`AuthConfig`]
//! without touching the filesystem. All settings are stored in `HashMap`s,
//! making it suitable for unit and integration tests that need deterministic
//! configuration without disk I/O.

use std::collections::HashMap;

use super::{AuthConfig, Config, default_for_key};

/// In-memory configuration for testing.
///
/// Stores all settings in `HashMap`s. No disk I/O is performed.
///
/// # Examples
///
/// ```
/// use ghc_core::config::MemoryConfig;
/// use ghc_core::config::Config;
///
/// let config = MemoryConfig::new()
///     .with_host("github.com", "testuser", "ghp_token123");
///
/// let auth = config.authentication();
/// let (token, source) = auth.active_token("github.com").unwrap();
/// assert_eq!(token, "ghp_token123");
/// assert_eq!(source, "config");
/// ```
#[derive(Debug, Default)]
pub struct MemoryConfig {
    global: HashMap<String, String>,
    host_settings: HashMap<String, HashMap<String, String>>,
    aliases: HashMap<String, String>,
    /// Auth storage: hostname -> (active_user, HashMap<username, token>)
    auth: HashMap<String, (String, HashMap<String, String>)>,
}

impl MemoryConfig {
    /// Create a new empty in-memory configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an authenticated host with the given username and token.
    ///
    /// The provided user becomes the active user for the host.
    #[must_use]
    pub fn with_host(mut self, hostname: &str, username: &str, token: &str) -> Self {
        let mut users = HashMap::new();
        users.insert(username.to_string(), token.to_string());
        self.auth
            .insert(hostname.to_string(), (username.to_string(), users));
        self
    }
}

impl Config for MemoryConfig {
    fn get(&self, hostname: &str, key: &str) -> Option<String> {
        // Check host-specific settings first
        if !hostname.is_empty()
            && let Some(host_map) = self.host_settings.get(hostname)
            && let Some(val) = host_map.get(key)
        {
            return Some(val.clone());
        }

        // Fall back to global settings
        self.global.get(key).cloned()
    }

    fn get_or_default(&self, hostname: &str, key: &str) -> String {
        self.get(hostname, key)
            .unwrap_or_else(|| default_for_key(key).to_string())
    }

    fn set(&mut self, hostname: &str, key: &str, value: &str) -> anyhow::Result<()> {
        if hostname.is_empty() {
            self.global.insert(key.to_string(), value.to_string());
        } else {
            self.host_settings
                .entry(hostname.to_string())
                .or_default()
                .insert(key.to_string(), value.to_string());
        }
        Ok(())
    }

    fn aliases(&self) -> &HashMap<String, String> {
        &self.aliases
    }

    fn set_alias(&mut self, name: &str, expansion: &str) {
        self.aliases.insert(name.to_string(), expansion.to_string());
    }

    fn delete_alias(&mut self, name: &str) -> Option<String> {
        self.aliases.remove(name)
    }

    fn hosts(&self) -> Vec<String> {
        self.auth.keys().cloned().collect()
    }

    fn authentication(&self) -> &dyn AuthConfig {
        self
    }

    fn authentication_mut(&mut self) -> &mut dyn AuthConfig {
        self
    }

    fn write(&self) -> anyhow::Result<()> {
        // No-op: in-memory config has nothing to persist.
        Ok(())
    }
}

impl AuthConfig for MemoryConfig {
    fn active_token(&self, hostname: &str) -> Option<(String, String)> {
        let (active_user, users) = self.auth.get(hostname)?;
        let token = users.get(active_user)?;
        Some((token.clone(), "config".to_string()))
    }

    fn active_user(&self, hostname: &str) -> Option<String> {
        let (active_user, _) = self.auth.get(hostname)?;
        Some(active_user.clone())
    }

    fn hosts(&self) -> Vec<String> {
        self.auth.keys().cloned().collect()
    }

    fn login(
        &mut self,
        hostname: &str,
        username: &str,
        token: &str,
        git_protocol: &str,
    ) -> anyhow::Result<()> {
        let entry = self
            .auth
            .entry(hostname.to_string())
            .or_insert_with(|| (String::new(), HashMap::new()));
        entry.0 = username.to_string();
        entry.1.insert(username.to_string(), token.to_string());

        if !git_protocol.is_empty() {
            self.host_settings
                .entry(hostname.to_string())
                .or_default()
                .insert("git_protocol".to_string(), git_protocol.to_string());
        }

        Ok(())
    }

    fn logout(&mut self, hostname: &str, username: &str) -> anyhow::Result<()> {
        if let Some((active_user, users)) = self.auth.get_mut(hostname) {
            users.remove(username);
            if users.is_empty() {
                self.auth.remove(hostname);
            } else if active_user == username {
                // Switch to the first remaining user
                if let Some(next_user) = users.keys().next().cloned() {
                    *active_user = next_user;
                }
            }
        }
        Ok(())
    }

    fn switch_user(&mut self, hostname: &str, username: &str) -> anyhow::Result<()> {
        let (active_user, users) = self
            .auth
            .get_mut(hostname)
            .ok_or_else(|| anyhow::anyhow!("not logged into {hostname}"))?;

        if !users.contains_key(username) {
            anyhow::bail!("user {username} not found for {hostname}");
        }

        *active_user = username.to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Construction ---

    #[test]
    fn test_should_create_empty_config() {
        let cfg = MemoryConfig::new();
        assert!(Config::hosts(&cfg).is_empty());
        assert!(cfg.aliases().is_empty());
    }

    #[test]
    fn test_should_create_config_with_host() {
        let cfg = MemoryConfig::new().with_host("github.com", "user1", "ghp_abc");
        let hosts = Config::hosts(&cfg);
        assert_eq!(hosts.len(), 1);
        assert!(hosts.contains(&"github.com".to_string()));
    }

    #[test]
    fn test_should_chain_multiple_hosts() {
        let cfg = MemoryConfig::new()
            .with_host("github.com", "user1", "token1")
            .with_host("ghe.io", "user2", "token2");
        let hosts = Config::hosts(&cfg);
        assert_eq!(hosts.len(), 2);
    }

    // --- Config::get / set ---

    #[test]
    fn test_should_return_none_for_unset_key() {
        let cfg = MemoryConfig::new();
        assert!(cfg.get("", "editor").is_none());
        assert!(cfg.get("github.com", "editor").is_none());
    }

    #[test]
    fn test_should_set_and_get_global_value() {
        let mut cfg = MemoryConfig::new();
        cfg.set("", "editor", "vim").unwrap();
        assert_eq!(cfg.get("", "editor"), Some("vim".to_string()));
    }

    #[test]
    fn test_should_set_and_get_host_specific_value() {
        let mut cfg = MemoryConfig::new();
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
        // Global remains unset
        assert!(cfg.get("", "git_protocol").is_none());
    }

    #[test]
    fn test_should_prefer_host_specific_over_global() {
        let mut cfg = MemoryConfig::new();
        cfg.set("", "git_protocol", "https").unwrap();
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
        assert_eq!(cfg.get("", "git_protocol"), Some("https".to_string()));
    }

    #[test]
    fn test_should_fall_back_to_global_when_host_has_no_key() {
        let mut cfg = MemoryConfig::new();
        cfg.set("", "editor", "vim").unwrap();
        // Host exists but has no editor setting
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        assert_eq!(cfg.get("github.com", "editor"), Some("vim".to_string()));
    }

    // --- Config::get_or_default ---

    #[test]
    fn test_should_return_default_for_git_protocol() {
        let cfg = MemoryConfig::new();
        assert_eq!(cfg.get_or_default("", "git_protocol"), "https");
    }

    #[test]
    fn test_should_return_default_for_prompt() {
        let cfg = MemoryConfig::new();
        assert_eq!(cfg.get_or_default("", "prompt"), "enabled");
    }

    #[test]
    fn test_should_return_empty_default_for_unknown_key() {
        let cfg = MemoryConfig::new();
        assert_eq!(cfg.get_or_default("", "nonexistent"), "");
    }

    // --- Default trait methods ---

    #[test]
    fn test_should_return_default_git_protocol_via_method() {
        let cfg = MemoryConfig::new();
        assert_eq!(cfg.git_protocol("github.com"), "https");
    }

    #[test]
    fn test_should_return_none_for_editor_on_empty() {
        let cfg = MemoryConfig::new();
        assert!(cfg.editor("").is_none());
    }

    #[test]
    fn test_should_return_none_for_pager_on_empty() {
        let cfg = MemoryConfig::new();
        assert!(cfg.pager("").is_none());
    }

    #[test]
    fn test_should_return_none_for_browser_on_empty() {
        let cfg = MemoryConfig::new();
        assert!(cfg.browser("").is_none());
    }

    #[test]
    fn test_should_return_default_prompt_via_method() {
        let cfg = MemoryConfig::new();
        assert_eq!(cfg.prompt(""), "enabled");
    }

    // --- Aliases ---

    #[test]
    fn test_should_set_and_get_alias() {
        let mut cfg = MemoryConfig::new();
        cfg.set_alias("co", "pr checkout");
        assert_eq!(cfg.aliases().get("co"), Some(&"pr checkout".to_string()));
    }

    #[test]
    fn test_should_delete_alias_and_return_old_value() {
        let mut cfg = MemoryConfig::new();
        cfg.set_alias("co", "pr checkout");
        let old = cfg.delete_alias("co");
        assert_eq!(old, Some("pr checkout".to_string()));
        assert!(cfg.aliases().is_empty());
    }

    #[test]
    fn test_should_return_none_when_deleting_nonexistent_alias() {
        let mut cfg = MemoryConfig::new();
        assert!(cfg.delete_alias("nope").is_none());
    }

    // --- AuthConfig ---

    #[test]
    fn test_should_return_active_token_from_with_host() {
        let cfg = MemoryConfig::new().with_host("github.com", "user1", "ghp_abc");
        let (token, source) = cfg.active_token("github.com").unwrap();
        assert_eq!(token, "ghp_abc");
        assert_eq!(source, "config");
    }

    #[test]
    fn test_should_return_active_user_from_with_host() {
        let cfg = MemoryConfig::new().with_host("github.com", "user1", "ghp_abc");
        assert_eq!(cfg.active_user("github.com"), Some("user1".to_string()));
    }

    #[test]
    fn test_should_return_none_for_unknown_host_token() {
        let cfg = MemoryConfig::new();
        assert!(cfg.active_token("unknown.host").is_none());
    }

    #[test]
    fn test_should_return_none_for_unknown_host_user() {
        let cfg = MemoryConfig::new();
        assert!(cfg.active_user("unknown.host").is_none());
    }

    #[test]
    fn test_should_login_and_retrieve_credentials() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "testuser", "ghp_test", "https")
            .unwrap();

        let auth = cfg.authentication();
        let (token, source) = auth.active_token("github.com").unwrap();
        assert_eq!(token, "ghp_test");
        assert_eq!(source, "config");
        assert_eq!(auth.active_user("github.com"), Some("testuser".to_string()));
    }

    #[test]
    fn test_should_set_git_protocol_on_login() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user", "token", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
    }

    #[test]
    fn test_should_not_set_git_protocol_when_empty_on_login() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user", "token", "").unwrap();
        assert!(cfg.get("github.com", "git_protocol").is_none());
    }

    #[test]
    fn test_should_logout_single_user_removes_host() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user1", "token1", "https").unwrap();
        cfg.logout("github.com", "user1").unwrap();

        assert!(cfg.active_token("github.com").is_none());
        assert!(AuthConfig::hosts(&cfg).is_empty());
    }

    #[test]
    fn test_should_logout_one_user_keeps_others() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user1", "token1", "https").unwrap();
        cfg.login("github.com", "user2", "token2", "").unwrap();
        cfg.logout("github.com", "user1").unwrap();

        // Host still exists with user2
        let hosts = AuthConfig::hosts(&cfg);
        assert_eq!(hosts.len(), 1);
        assert!(cfg.active_token("github.com").is_some());
    }

    #[test]
    fn test_should_switch_user() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user1", "token1", "https").unwrap();
        cfg.login("github.com", "user2", "token2", "").unwrap();
        assert_eq!(cfg.active_user("github.com"), Some("user2".to_string()));

        cfg.switch_user("github.com", "user1").unwrap();
        assert_eq!(cfg.active_user("github.com"), Some("user1".to_string()));
        let (token, _) = cfg.active_token("github.com").unwrap();
        assert_eq!(token, "token1");
    }

    #[test]
    fn test_should_error_switching_to_unknown_user() {
        let mut cfg = MemoryConfig::new();
        cfg.login("github.com", "user1", "token1", "https").unwrap();
        let result = cfg.switch_user("github.com", "ghost");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_should_error_switching_on_unknown_host() {
        let mut cfg = MemoryConfig::new();
        let result = cfg.switch_user("unknown.host", "user");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged into"));
    }

    // --- write() ---

    #[test]
    fn test_should_write_without_error() {
        let cfg = MemoryConfig::new().with_host("github.com", "u", "t");
        assert!(cfg.write().is_ok());
    }

    // --- Auth hosts matches Config hosts ---

    #[test]
    fn test_should_have_consistent_hosts_between_config_and_auth() {
        let cfg = MemoryConfig::new()
            .with_host("github.com", "u1", "t1")
            .with_host("ghe.io", "u2", "t2");
        let config_hosts = Config::hosts(&cfg);
        let auth_hosts = AuthConfig::hosts(&cfg);
        assert_eq!(config_hosts.len(), auth_hosts.len());
        for host in &config_hosts {
            assert!(auth_hosts.contains(host));
        }
    }

    // --- Logout of nonexistent host is a no-op ---

    #[test]
    fn test_should_logout_nonexistent_host_without_error() {
        let mut cfg = MemoryConfig::new();
        assert!(cfg.logout("unknown.host", "user").is_ok());
    }

    // --- authentication() / authentication_mut() ---

    #[test]
    fn test_should_return_self_as_auth_config() {
        let mut cfg = MemoryConfig::new().with_host("github.com", "user", "token");
        // Use trait object to verify it works through the indirection
        let auth: &dyn AuthConfig = cfg.authentication();
        assert_eq!(auth.active_user("github.com"), Some("user".to_string()));

        let auth_mut: &mut dyn AuthConfig = cfg.authentication_mut();
        auth_mut.login("ghe.io", "user2", "token2", "ssh").unwrap();
        assert_eq!(
            cfg.authentication().active_user("ghe.io"),
            Some("user2".to_string()),
        );
    }
}
