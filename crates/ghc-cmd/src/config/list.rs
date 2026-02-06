//! `ghc config list` command.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_println;

use crate::factory::Factory;

/// Print a list of configuration keys and values.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Get per-host configuration.
    #[arg(short = 'h', long)]
    host: Option<String>,
}

impl ListArgs {
    /// Run the config list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration cannot be read.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let hostname = self.host.as_deref().unwrap_or("");

        for option in ghc_core::config::CONFIG_OPTIONS {
            let value = option.current_value(&**cfg, hostname);
            ios_println!(ios, "{}={value}", option.key);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_list_config_options() {
        let h = TestHarness::new().await;
        let args = ListArgs { host: None };
        args.run(&h.factory).unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("git_protocol="));
        assert!(stdout.contains("prompt="));
    }

    #[tokio::test]
    async fn test_should_list_config_with_defaults() {
        let h = TestHarness::new().await;
        let args = ListArgs { host: None };
        args.run(&h.factory).unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("git_protocol=https"));
        assert!(stdout.contains("prompt=enabled"));
    }
}
