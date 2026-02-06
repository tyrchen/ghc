//! Configuration system for GHC.
//!
//! Maps from Go's `internal/config/` and `internal/gh/` packages.
//! Manages config.yml, hosts.yml, and state.yml files.

mod file_config;
mod memory_config;

use std::collections::HashMap;

pub use file_config::FileConfig;
pub use memory_config::MemoryConfig;

/// Configuration directory path (usually ~/.config/gh).
pub fn config_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("GH_CONFIG_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::config_dir().map_or_else(
        || {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".config")
                .join("gh")
        },
        |d| d.join("gh"),
    )
}

/// State directory path (usually same as config dir).
pub fn state_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("GH_STATE_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::state_dir().map_or_else(config_dir, |d| d.join("gh"))
}

/// Data directory path.
pub fn data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("GH_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::data_dir().map_or_else(config_dir, |d| d.join("gh"))
}

/// Configuration trait for accessing settings.
pub trait Config: Send + Sync + std::fmt::Debug {
    /// Get a config value, checking hostname scope first then global.
    fn get(&self, hostname: &str, key: &str) -> Option<String>;

    /// Get a config value with its default.
    fn get_or_default(&self, hostname: &str, key: &str) -> String;

    /// Set a config value. Empty hostname means global.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be updated.
    fn set(&mut self, hostname: &str, key: &str, value: &str) -> anyhow::Result<()>;

    /// Get the git protocol preference for a host.
    fn git_protocol(&self, hostname: &str) -> String {
        self.get_or_default(hostname, "git_protocol")
    }

    /// Get the editor preference.
    fn editor(&self, hostname: &str) -> Option<String> {
        self.get(hostname, "editor")
    }

    /// Get the pager preference.
    fn pager(&self, hostname: &str) -> Option<String> {
        self.get(hostname, "pager")
    }

    /// Get the browser preference.
    fn browser(&self, hostname: &str) -> Option<String> {
        self.get(hostname, "browser")
    }

    /// Get prompt setting.
    fn prompt(&self, hostname: &str) -> String {
        self.get_or_default(hostname, "prompt")
    }

    /// Get aliases.
    fn aliases(&self) -> &HashMap<String, String>;

    /// Set an alias.
    fn set_alias(&mut self, name: &str, expansion: &str);

    /// Delete an alias. Returns the old expansion if the alias existed.
    fn delete_alias(&mut self, name: &str) -> Option<String>;

    /// Get the list of authenticated hosts.
    fn hosts(&self) -> Vec<String>;

    /// Get authentication configuration.
    fn authentication(&self) -> &dyn AuthConfig;

    /// Get mutable authentication configuration.
    fn authentication_mut(&mut self) -> &mut dyn AuthConfig;

    /// Write config to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be saved.
    fn write(&self) -> anyhow::Result<()>;
}

/// Authentication configuration trait.
pub trait AuthConfig: Send + Sync + std::fmt::Debug {
    /// Get the active token for a hostname. Returns (token, source).
    fn active_token(&self, hostname: &str) -> Option<(String, String)>;

    /// Get the active username for a hostname.
    fn active_user(&self, hostname: &str) -> Option<String>;

    /// Get all configured hostnames.
    fn hosts(&self) -> Vec<String>;

    /// Store authentication credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be stored.
    fn login(
        &mut self,
        hostname: &str,
        username: &str,
        token: &str,
        git_protocol: &str,
    ) -> anyhow::Result<()>;

    /// Remove authentication credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be removed.
    fn logout(&mut self, hostname: &str, username: &str) -> anyhow::Result<()>;

    /// Switch the active user for a hostname.
    ///
    /// # Errors
    ///
    /// Returns an error if the user cannot be switched.
    fn switch_user(&mut self, hostname: &str, username: &str) -> anyhow::Result<()>;
}

/// Cache directory path.
pub fn cache_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("GH_CACHE_DIR") {
        return std::path::PathBuf::from(dir);
    }
    dirs::cache_dir().map_or_else(config_dir, |d| d.join("gh"))
}

/// Known configuration keys with descriptions and allowed values.
pub static CONFIG_OPTIONS: &[ConfigOption] = &[
    ConfigOption {
        key: "git_protocol",
        description: "the protocol to use for git clone and push operations",
        allowed_values: &["https", "ssh"],
        default_value: "https",
    },
    ConfigOption {
        key: "editor",
        description: "the text editor program to use for authoring text",
        allowed_values: &[],
        default_value: "",
    },
    ConfigOption {
        key: "prompt",
        description: "toggle interactive prompting in the terminal",
        allowed_values: &["enabled", "disabled"],
        default_value: "enabled",
    },
    ConfigOption {
        key: "pager",
        description: "the terminal pager program to send standard output to",
        allowed_values: &[],
        default_value: "",
    },
    ConfigOption {
        key: "browser",
        description: "the web browser to use for opening URLs",
        allowed_values: &[],
        default_value: "",
    },
    ConfigOption {
        key: "http_unix_socket",
        description: "the path to a Unix domain socket through which to make an HTTP connection",
        allowed_values: &[],
        default_value: "",
    },
];

/// A known configuration option.
#[derive(Debug)]
pub struct ConfigOption {
    /// Config key name.
    pub key: &'static str,
    /// Description of what this option does.
    pub description: &'static str,
    /// Valid values, empty means any string.
    pub allowed_values: &'static [&'static str],
    /// Default value.
    pub default_value: &'static str,
}

impl ConfigOption {
    /// Get the current value from config, or the default.
    pub fn current_value(&self, config: &dyn Config, hostname: &str) -> String {
        config.get_or_default(hostname, self.key)
    }
}

/// Default configuration values.
pub fn default_for_key(key: &str) -> &str {
    match key {
        "git_protocol" => "https",
        "prompt" => "enabled",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("git_protocol", "https")]
    #[case("editor", "")]
    #[case("prompt", "enabled")]
    #[case("pager", "")]
    #[case("browser", "")]
    #[case("http_unix_socket", "")]
    #[case("unknown_key", "")]
    #[case("", "")]
    fn test_should_return_defaults(#[case] key: &str, #[case] expected: &str) {
        assert_eq!(default_for_key(key), expected);
    }

    #[test]
    fn test_should_return_config_dir() {
        let dir = config_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_should_return_state_dir() {
        let dir = state_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_should_return_data_dir() {
        let dir = data_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_should_use_env_var_for_config_dir() {
        let _guard = EnvVarGuard::set("GH_CONFIG_DIR", "/tmp/test-gh-config");
        let dir = config_dir();
        assert_eq!(dir, std::path::PathBuf::from("/tmp/test-gh-config"));
    }

    #[test]
    fn test_should_use_env_var_for_state_dir() {
        let _guard = EnvVarGuard::set("GH_STATE_DIR", "/tmp/test-gh-state");
        let dir = state_dir();
        assert_eq!(dir, std::path::PathBuf::from("/tmp/test-gh-state"));
    }

    #[test]
    fn test_should_use_env_var_for_data_dir() {
        let _guard = EnvVarGuard::set("GH_DATA_DIR", "/tmp/test-gh-data");
        let dir = data_dir();
        assert_eq!(dir, std::path::PathBuf::from("/tmp/test-gh-data"));
    }

    /// RAII guard for environment variables in tests.
    struct EnvVarGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests are run single-threaded with --test-threads=1
            // when env vars are involved, avoiding data races.
            unsafe { std::env::set_var(key, value) };
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: See EnvVarGuard::set
                Some(val) => unsafe { std::env::set_var(&self.key, val) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }
}
