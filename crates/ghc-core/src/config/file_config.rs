//! File-based configuration implementation.
//!
//! Reads/writes config.yml and hosts.yml in the GH config directory.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{AuthConfig, Config, config_dir, default_for_key};
use crate::errors::ConfigError;

/// File-based configuration backed by YAML files.
#[derive(Debug)]
pub struct FileConfig {
    config_path: PathBuf,
    hosts_path: PathBuf,
    global: ConfigData,
    hosts: HashMap<String, HostConfig>,
    aliases: HashMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigData {
    #[serde(default)]
    git_protocol: Option<String>,
    #[serde(default)]
    editor: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    pager: Option<String>,
    #[serde(default)]
    browser: Option<String>,
    #[serde(default)]
    http_unix_socket: Option<String>,
    #[serde(default)]
    aliases: HashMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HostConfig {
    #[serde(default)]
    oauth_token: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    git_protocol: Option<String>,
    #[serde(default)]
    users: HashMap<String, UserEntry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UserEntry {
    #[serde(default)]
    oauth_token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HostsFile {
    #[serde(flatten)]
    hosts: HashMap<String, HostConfig>,
}

impl FileConfig {
    /// Load configuration from disk.
    ///
    /// # Errors
    ///
    /// Returns an error if config files cannot be read or parsed.
    pub fn load() -> anyhow::Result<Self> {
        let dir = config_dir();
        let config_path = dir.join("config.yml");
        let hosts_path = dir.join("hosts.yml");

        let global = if config_path.exists() {
            let content = fs::read_to_string(&config_path).map_err(|e| ConfigError::ReadFile {
                path: config_path.display().to_string(),
                source: e,
            })?;
            if content.trim().is_empty() {
                ConfigData::default()
            } else {
                serde_yaml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))?
            }
        } else {
            ConfigData::default()
        };

        let hosts = if hosts_path.exists() {
            let content = fs::read_to_string(&hosts_path).map_err(|e| ConfigError::ReadFile {
                path: hosts_path.display().to_string(),
                source: e,
            })?;
            if content.trim().is_empty() {
                HashMap::new()
            } else {
                let hosts_file: HostsFile = serde_yaml::from_str(&content)
                    .map_err(|e| ConfigError::Parse(e.to_string()))?;
                hosts_file.hosts
            }
        } else {
            HashMap::new()
        };

        let aliases = global.aliases.clone();

        Ok(Self {
            config_path,
            hosts_path,
            global,
            hosts,
            aliases,
        })
    }

    /// Create an empty in-memory config (for testing).
    pub fn empty() -> Self {
        Self {
            config_path: PathBuf::from("/dev/null"),
            hosts_path: PathBuf::from("/dev/null"),
            global: ConfigData::default(),
            hosts: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    fn get_host_value(&self, hostname: &str, key: &str) -> Option<String> {
        let host = self.hosts.get(hostname)?;
        match key {
            "oauth_token" => host.oauth_token.clone(),
            "user" => host.user.clone(),
            "git_protocol" => host.git_protocol.clone(),
            _ => None,
        }
    }

    fn get_global_value(&self, key: &str) -> Option<String> {
        match key {
            "git_protocol" => self.global.git_protocol.clone(),
            "editor" => self.global.editor.clone(),
            "prompt" => self.global.prompt.clone(),
            "pager" => self.global.pager.clone(),
            "browser" => self.global.browser.clone(),
            "http_unix_socket" => self.global.http_unix_socket.clone(),
            _ => None,
        }
    }
}

impl Config for FileConfig {
    fn get(&self, hostname: &str, key: &str) -> Option<String> {
        // Check environment variables first
        let env_key = format!("GH_{}", key.to_uppercase());
        if let Ok(val) = std::env::var(&env_key) {
            return Some(val);
        }

        // Check host-specific config
        if !hostname.is_empty()
            && let Some(val) = self.get_host_value(hostname, key)
        {
            return Some(val);
        }

        // Check global config
        self.get_global_value(key)
    }

    fn get_or_default(&self, hostname: &str, key: &str) -> String {
        self.get(hostname, key)
            .unwrap_or_else(|| default_for_key(key).to_string())
    }

    fn set(&mut self, hostname: &str, key: &str, value: &str) -> anyhow::Result<()> {
        if hostname.is_empty() {
            match key {
                "git_protocol" => self.global.git_protocol = Some(value.to_string()),
                "editor" => self.global.editor = Some(value.to_string()),
                "prompt" => self.global.prompt = Some(value.to_string()),
                "pager" => self.global.pager = Some(value.to_string()),
                "browser" => self.global.browser = Some(value.to_string()),
                "http_unix_socket" => self.global.http_unix_socket = Some(value.to_string()),
                _ => {}
            }
        } else {
            let host = self.hosts.entry(hostname.to_string()).or_default();
            match key {
                "oauth_token" => host.oauth_token = Some(value.to_string()),
                "user" => host.user = Some(value.to_string()),
                "git_protocol" => host.git_protocol = Some(value.to_string()),
                _ => {}
            }
        }
        Ok(())
    }

    fn aliases(&self) -> &HashMap<String, String> {
        &self.aliases
    }

    fn set_alias(&mut self, name: &str, expansion: &str) {
        self.aliases.insert(name.to_string(), expansion.to_string());
        self.global
            .aliases
            .insert(name.to_string(), expansion.to_string());
    }

    fn delete_alias(&mut self, name: &str) -> Option<String> {
        self.global.aliases.remove(name);
        self.aliases.remove(name)
    }

    fn hosts(&self) -> Vec<String> {
        self.hosts.keys().cloned().collect()
    }

    fn authentication(&self) -> &dyn AuthConfig {
        self
    }

    fn authentication_mut(&mut self) -> &mut dyn AuthConfig {
        self
    }

    fn write(&self) -> anyhow::Result<()> {
        let dir = self.config_path.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "config path has no parent directory: {}",
                self.config_path.display()
            )
        })?;
        fs::create_dir_all(dir)?;

        let config_yaml =
            serde_yaml::to_string(&self.global).map_err(|e| ConfigError::Parse(e.to_string()))?;
        fs::write(&self.config_path, config_yaml).map_err(|e| ConfigError::WriteFile {
            path: self.config_path.display().to_string(),
            source: e,
        })?;

        let hosts_yaml =
            serde_yaml::to_string(&self.hosts).map_err(|e| ConfigError::Parse(e.to_string()))?;
        fs::write(&self.hosts_path, hosts_yaml).map_err(|e| ConfigError::WriteFile {
            path: self.hosts_path.display().to_string(),
            source: e,
        })?;

        Ok(())
    }
}

impl AuthConfig for FileConfig {
    fn active_token(&self, hostname: &str) -> Option<(String, String)> {
        // Check environment first
        if let Ok(token) = std::env::var("GH_TOKEN") {
            return Some((token, "GH_TOKEN".to_string()));
        }
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            return Some((token, "GITHUB_TOKEN".to_string()));
        }

        // Check keyring before config file (matches gh CLI behavior)
        if let Ok(Some(token)) = crate::keyring_store::get_token(hostname) {
            return Some((token, "keyring".to_string()));
        }

        let host = self.hosts.get(hostname)?;
        let token = host.oauth_token.as_ref()?;
        Some((token.clone(), "config".to_string()))
    }

    fn active_user(&self, hostname: &str) -> Option<String> {
        self.hosts.get(hostname)?.user.clone()
    }

    fn hosts(&self) -> Vec<String> {
        self.hosts.keys().cloned().collect()
    }

    fn login(
        &mut self,
        hostname: &str,
        username: &str,
        token: &str,
        git_protocol: &str,
    ) -> anyhow::Result<()> {
        let host = self.hosts.entry(hostname.to_string()).or_default();
        host.oauth_token = Some(token.to_string());
        host.user = Some(username.to_string());
        if !git_protocol.is_empty() {
            host.git_protocol = Some(git_protocol.to_string());
        }
        self.write()
    }

    fn logout(&mut self, hostname: &str, _username: &str) -> anyhow::Result<()> {
        self.hosts.remove(hostname);
        self.write()
    }

    fn switch_user(&mut self, hostname: &str, username: &str) -> anyhow::Result<()> {
        let host = self
            .hosts
            .get_mut(hostname)
            .ok_or_else(|| anyhow::anyhow!("not logged into {hostname}"))?;

        // Check if user exists in the users map and swap tokens
        let new_token = host
            .users
            .get(username)
            .and_then(|e| e.oauth_token.clone())
            .ok_or_else(|| anyhow::anyhow!("user {username} not found for {hostname}"))?;

        // Save current user's token
        let current_user = host.user.clone().unwrap_or_default();
        let current_token = host.oauth_token.clone().unwrap_or_default();

        if !current_user.is_empty() && !current_token.is_empty() {
            host.users.entry(current_user).or_default().oauth_token = Some(current_token);
        }

        host.oauth_token = Some(new_token);
        host.user = Some(username.to_string());

        self.write()
    }

    fn users_for_host(&self, hostname: &str) -> Vec<String> {
        let Some(host) = self.hosts.get(hostname) else {
            return Vec::new();
        };
        let mut users: Vec<String> = host.users.keys().cloned().collect();
        // Include the active user if not already in the users map
        if let Some(ref active) = host.user
            && !users.contains(active)
        {
            users.push(active.clone());
        }
        users
    }

    fn token_for_user(&self, hostname: &str, username: &str) -> Option<(String, String)> {
        let host = self.hosts.get(hostname)?;
        // Check if it's the active user first
        if host.user.as_deref() == Some(username) {
            // Check keyring first (matches gh CLI behavior)
            if let Ok(Some(token)) = crate::keyring_store::get_token(hostname) {
                return Some((token, "keyring".to_string()));
            }
            let token = host.oauth_token.as_ref()?;
            return Some((token.clone(), "config".to_string()));
        }
        // Check the users map
        let entry = host.users.get(username)?;
        let token = entry.oauth_token.as_ref()?;
        Some((token.clone(), "config".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::EnvVarGuard;

    // --- Empty config ---

    #[test]
    fn test_should_create_empty_config() {
        let cfg = FileConfig::empty();
        assert!(Config::hosts(&cfg).is_empty());
        assert_eq!(cfg.get_or_default("", "git_protocol"), "https");
        assert_eq!(cfg.get_or_default("", "prompt"), "enabled");
    }

    #[test]
    fn test_should_return_none_for_unset_keys_on_empty_config() {
        let cfg = FileConfig::empty();
        assert!(cfg.get("", "editor").is_none());
        assert!(cfg.get("", "pager").is_none());
        assert!(cfg.get("", "browser").is_none());
    }

    // --- Global config ---

    #[test]
    fn test_should_set_and_get_global() {
        let mut cfg = FileConfig::empty();
        cfg.set("", "editor", "vim").unwrap();
        assert_eq!(cfg.get("", "editor"), Some("vim".to_string()));
    }

    #[test]
    fn test_should_set_and_get_all_global_keys() {
        let mut cfg = FileConfig::empty();
        cfg.set("", "git_protocol", "ssh").unwrap();
        cfg.set("", "editor", "nvim").unwrap();
        cfg.set("", "prompt", "disabled").unwrap();
        cfg.set("", "pager", "less -R").unwrap();
        cfg.set("", "browser", "firefox").unwrap();
        cfg.set("", "http_unix_socket", "/tmp/sock").unwrap();

        assert_eq!(cfg.get("", "git_protocol"), Some("ssh".to_string()));
        assert_eq!(cfg.get("", "editor"), Some("nvim".to_string()));
        assert_eq!(cfg.get("", "prompt"), Some("disabled".to_string()));
        assert_eq!(cfg.get("", "pager"), Some("less -R".to_string()));
        assert_eq!(cfg.get("", "browser"), Some("firefox".to_string()));
        assert_eq!(
            cfg.get("", "http_unix_socket"),
            Some("/tmp/sock".to_string())
        );
    }

    #[test]
    fn test_should_ignore_unknown_global_key() {
        let mut cfg = FileConfig::empty();
        cfg.set("", "unknown_key", "value").unwrap();
        assert!(cfg.get("", "unknown_key").is_none());
    }

    // --- Host-specific config ---

    #[test]
    fn test_should_set_and_get_host_specific() {
        let mut cfg = FileConfig::empty();
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
        // Global should still be default
        assert_eq!(cfg.get("", "git_protocol"), None);
    }

    #[test]
    fn test_should_override_global_with_host_specific() {
        let mut cfg = FileConfig::empty();
        cfg.set("", "git_protocol", "https").unwrap();
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
        // Global unchanged
        assert_eq!(cfg.get("", "git_protocol"), Some("https".to_string()),);
    }

    #[test]
    fn test_should_fall_back_to_global_when_host_not_set() {
        let mut cfg = FileConfig::empty();
        cfg.set("", "git_protocol", "ssh").unwrap();
        // No host-specific setting for ghe.io -> falls back to global
        assert_eq!(cfg.get("ghe.io", "git_protocol"), Some("ssh".to_string()),);
    }

    // --- Config trait methods ---

    #[test]
    fn test_should_return_git_protocol_default() {
        let cfg = FileConfig::empty();
        assert_eq!(cfg.git_protocol("github.com"), "https");
    }

    #[test]
    fn test_should_return_prompt_default() {
        let cfg = FileConfig::empty();
        assert_eq!(cfg.prompt(""), "enabled");
    }

    #[test]
    fn test_should_return_none_for_editor_on_empty() {
        let cfg = FileConfig::empty();
        assert!(cfg.editor("").is_none());
    }

    #[test]
    fn test_should_return_none_for_pager_on_empty() {
        let cfg = FileConfig::empty();
        assert!(cfg.pager("").is_none());
    }

    #[test]
    fn test_should_return_none_for_browser_on_empty() {
        let cfg = FileConfig::empty();
        assert!(cfg.browser("").is_none());
    }

    #[test]
    fn test_should_return_empty_aliases_on_empty() {
        let cfg = FileConfig::empty();
        assert!(cfg.aliases().is_empty());
    }

    // --- Auth ---

    #[test]
    fn test_should_login_and_get_token() {
        let mut cfg = FileConfig::empty();
        cfg.login("github.com", "testuser", "ghp_test123", "https")
            .unwrap();

        let auth = cfg.authentication();
        let (token, source) = auth.active_token("github.com").unwrap();
        assert_eq!(token, "ghp_test123");
        assert_eq!(source, "config");
        assert_eq!(auth.active_user("github.com"), Some("testuser".to_string()));
    }

    #[test]
    fn test_should_return_none_for_unknown_host() {
        let cfg = FileConfig::empty();
        let auth = cfg.authentication();
        assert!(auth.active_token("unknown.host").is_none());
        assert!(auth.active_user("unknown.host").is_none());
    }

    #[test]
    fn test_should_logout_and_remove_host() {
        let mut cfg = FileConfig::empty();
        cfg.login("github.com", "user", "token", "https").unwrap();
        assert!(!AuthConfig::hosts(&cfg).is_empty());

        cfg.logout("github.com", "user").unwrap();
        assert!(cfg.authentication().active_token("github.com").is_none());
        assert!(AuthConfig::hosts(&cfg).is_empty());
    }

    #[test]
    fn test_should_login_sets_git_protocol() {
        let mut cfg = FileConfig::empty();
        cfg.login("github.com", "user", "token", "ssh").unwrap();
        assert_eq!(
            cfg.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
    }

    #[test]
    fn test_should_not_set_git_protocol_when_empty() {
        let mut cfg = FileConfig::empty();
        cfg.login("github.com", "user", "token", "").unwrap();
        assert!(cfg.get("github.com", "git_protocol").is_none());
    }

    #[test]
    fn test_should_list_hosts_after_login() {
        let mut cfg = FileConfig::empty();
        cfg.login("github.com", "user1", "token1", "https").unwrap();
        cfg.login("ghe.io", "user2", "token2", "ssh").unwrap();

        let hosts = Config::hosts(&cfg);
        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains(&"github.com".to_string()));
        assert!(hosts.contains(&"ghe.io".to_string()));
    }

    // --- File-based load/write ---

    /// This test uses env vars which are process-global and can race
    /// with other tests in parallel. Run it with `--test-threads=1`
    /// if it fails intermittently.
    #[test]
    #[ignore = "requires filesystem"]
    fn test_should_load_and_write_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let hosts_path = dir.path().join("hosts.yml");

        // Write initial config
        std::fs::write(&config_path, "git_protocol: ssh\neditor: vim\n").unwrap();
        std::fs::write(&hosts_path, "").unwrap();

        // Manually set env to override config dir for this test
        let _guard = EnvVarGuard::set("GH_CONFIG_DIR", dir.path().to_str().unwrap());

        let cfg = FileConfig::load().unwrap();
        assert_eq!(cfg.get("", "git_protocol"), Some("ssh".to_string()));
        assert_eq!(cfg.get("", "editor"), Some("vim".to_string()));
    }

    #[test]
    #[ignore = "requires filesystem"]
    fn test_should_load_empty_config_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yml"), "").unwrap();
        std::fs::write(dir.path().join("hosts.yml"), "").unwrap();

        let _guard = EnvVarGuard::set("GH_CONFIG_DIR", dir.path().to_str().unwrap());

        let cfg = FileConfig::load().unwrap();
        assert_eq!(cfg.get_or_default("", "git_protocol"), "https");
    }

    #[test]
    #[ignore = "requires filesystem"]
    fn test_should_load_hosts_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yml"), "").unwrap();
        std::fs::write(
            dir.path().join("hosts.yml"),
            "github.com:\n  oauth_token: ghp_abc\n  user: testuser\n",
        )
        .unwrap();

        let _guard = EnvVarGuard::set("GH_CONFIG_DIR", dir.path().to_str().unwrap());

        let cfg = FileConfig::load().unwrap();
        let auth = cfg.authentication();
        let (token, source) = auth.active_token("github.com").unwrap();
        assert_eq!(token, "ghp_abc");
        assert_eq!(source, "config");
        assert_eq!(auth.active_user("github.com"), Some("testuser".to_string()));
    }

    #[test]
    #[ignore = "requires filesystem"]
    fn test_should_round_trip_write_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.yml");
        let hosts_path = dir.path().join("hosts.yml");

        // Create a config, modify it, write it, then re-load
        let mut cfg = FileConfig {
            config_path: config_path.clone(),
            hosts_path: hosts_path.clone(),
            global: ConfigData::default(),
            hosts: HashMap::new(),
            aliases: HashMap::new(),
        };

        cfg.set("", "editor", "code").unwrap();
        cfg.set("github.com", "git_protocol", "ssh").unwrap();
        cfg.set("github.com", "oauth_token", "ghp_xyz").unwrap();
        cfg.set("github.com", "user", "myuser").unwrap();
        cfg.write().unwrap();

        // Re-load
        let _guard = EnvVarGuard::set("GH_CONFIG_DIR", dir.path().to_str().unwrap());
        let cfg2 = FileConfig::load().unwrap();
        assert_eq!(cfg2.get("", "editor"), Some("code".to_string()));
        assert_eq!(
            cfg2.get("github.com", "git_protocol"),
            Some("ssh".to_string()),
        );
    }
}
