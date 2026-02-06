//! `ghc config clear-cache` command.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_println;

use crate::factory::Factory;

/// Clear the CLI cache.
#[derive(Debug, Args)]
pub struct ClearCacheArgs;

impl ClearCacheArgs {
    /// Run the config clear-cache command.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be removed.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let cache_dir = ghc_core::config::cache_dir();
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)?;
        }
        let cs = ios.color_scheme();
        ios_println!(ios, "{} Cleared the cache", cs.success_icon());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_clear_cache_successfully() {
        let h = TestHarness::new().await;
        let args = ClearCacheArgs;
        args.run(&h.factory).unwrap();
        assert!(h.stdout().contains("Cleared the cache"));
    }
}
