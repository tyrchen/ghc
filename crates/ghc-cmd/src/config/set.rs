//! `ghc config set` command.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Update configuration with a value for the given key.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The configuration key to set.
    key: String,
    /// The value to set.
    value: String,
    /// Set per-host setting.
    #[arg(short = 'h', long)]
    host: Option<String>,
}

impl SetArgs {
    /// Run the config set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is invalid or cannot be saved.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;

        // Validate the key
        let known = ghc_core::config::CONFIG_OPTIONS
            .iter()
            .any(|o| o.key == self.key);
        if !known {
            ios_eprintln!(
                ios,
                "! warning: '{}' is not a known configuration key",
                self.key
            );
        }

        // Validate the value if allowed values are specified
        if let Some(option) = ghc_core::config::CONFIG_OPTIONS
            .iter()
            .find(|o| o.key == self.key)
            && !option.allowed_values.is_empty()
            && !option.allowed_values.contains(&self.value.as_str())
        {
            let valid: Vec<String> = option
                .allowed_values
                .iter()
                .map(|v| format!("'{v}'"))
                .collect();
            anyhow::bail!(
                "failed to set {:?} to {:?}: valid values are {}",
                self.key,
                self.value,
                valid.join(", "),
            );
        }

        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let hostname = self.host.as_deref().unwrap_or("");
        cfg.set(hostname, &self.key, &self.value)?;
        cfg.write()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_set_known_config_value() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            key: "git_protocol".to_string(),
            value: "ssh".to_string(),
            host: None,
        };
        args.run(&h.factory).unwrap();
        // No output expected on success
        assert!(h.stdout().is_empty());
    }

    #[tokio::test]
    async fn test_should_warn_for_unknown_key() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            key: "unknown_key".to_string(),
            value: "something".to_string(),
            host: None,
        };
        args.run(&h.factory).unwrap();
        assert!(h.stderr().contains("warning"));
        assert!(h.stderr().contains("unknown_key"));
    }

    #[tokio::test]
    async fn test_should_error_for_invalid_value() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            key: "git_protocol".to_string(),
            value: "invalid".to_string(),
            host: None,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("valid values"));
    }

    #[tokio::test]
    async fn test_should_set_host_specific_value() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            key: "git_protocol".to_string(),
            value: "ssh".to_string(),
            host: Some("github.com".to_string()),
        };
        args.run(&h.factory).unwrap();
        assert!(h.stdout().is_empty());
    }
}
