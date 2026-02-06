//! `ghc alias list` command.

use std::collections::BTreeMap;

use anyhow::Result;
use clap::Args;

use ghc_core::ios_print;

use crate::factory::Factory;

/// List configured aliases.
#[derive(Debug, Args)]
pub struct ListArgs;

impl ListArgs {
    /// Run the alias list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the aliases cannot be read.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let aliases = cfg.aliases();

        if aliases.is_empty() {
            anyhow::bail!("no aliases configured");
        }

        // Sort for deterministic output
        let sorted: BTreeMap<_, _> = aliases.iter().collect();
        let yaml = serde_yaml::to_string(&sorted)
            .map_err(|e| anyhow::anyhow!("failed to serialize aliases: {e}"))?;
        ios_print!(ios, "{yaml}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_list_aliases() {
        let h = TestHarness::new().await;
        {
            let cfg_lock = h.factory.config().unwrap();
            let mut cfg = cfg_lock.lock().unwrap();
            cfg.set_alias("co", "pr checkout");
            cfg.set_alias("iv", "issue view");
        }

        let args = ListArgs;
        args.run(&h.factory).unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("co"));
        assert!(stdout.contains("pr checkout"));
        assert!(stdout.contains("iv"));
        assert!(stdout.contains("issue view"));
    }

    #[tokio::test]
    async fn test_should_error_when_no_aliases() {
        let h = TestHarness::new().await;
        let args = ListArgs;
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no aliases"));
    }
}
