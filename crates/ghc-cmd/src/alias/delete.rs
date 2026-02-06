//! `ghc alias delete` command.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Delete set aliases.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// The alias to delete.
    name: Option<String>,
    /// Delete all aliases.
    #[arg(long)]
    all: bool,
}

impl DeleteArgs {
    /// Run the alias delete command.
    ///
    /// # Errors
    ///
    /// Returns an error if the alias cannot be deleted.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;

        if self.name.is_none() && !self.all {
            anyhow::bail!("specify an alias to delete or `--all`");
        }
        if self.name.is_some() && self.all {
            anyhow::bail!("cannot use `--all` with alias name");
        }

        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let to_delete: Vec<(String, String)> = if self.all {
            let aliases = cfg.aliases().clone();
            if aliases.is_empty() {
                anyhow::bail!("no aliases configured");
            }
            let mut sorted: Vec<_> = aliases.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            sorted
        } else {
            let name = self.name.as_deref().unwrap_or("");
            let expansion = cfg
                .aliases()
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no such alias {name}"))?;
            vec![(name.to_string(), expansion)]
        };

        for (name, _) in &to_delete {
            cfg.delete_alias(name);
        }

        cfg.write()?;

        if ios.is_stdout_tty() {
            let cs = ios.color_scheme();
            for (name, expansion) in &to_delete {
                ios_eprintln!(
                    ios,
                    "{} Deleted alias {}; was {}",
                    cs.error_icon(),
                    name,
                    expansion,
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_delete_alias() {
        let h = TestHarness::new().await;
        // Set an alias first
        {
            let cfg_lock = h.factory.config().unwrap();
            let mut cfg = cfg_lock.lock().unwrap();
            cfg.set_alias("co", "pr checkout");
        }

        let args = DeleteArgs {
            name: Some("co".to_string()),
            all: false,
        };
        args.run(&h.factory).unwrap();

        let cfg_lock = h.factory.config().unwrap();
        let cfg = cfg_lock.lock().unwrap();
        assert!(cfg.aliases().is_empty());
    }

    #[tokio::test]
    async fn test_should_error_deleting_nonexistent_alias() {
        let h = TestHarness::new().await;
        let args = DeleteArgs {
            name: Some("nonexistent".to_string()),
            all: false,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no such alias"));
    }

    #[tokio::test]
    async fn test_should_error_without_name_or_all() {
        let h = TestHarness::new().await;
        let args = DeleteArgs {
            name: None,
            all: false,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("specify an alias"));
    }

    #[tokio::test]
    async fn test_should_delete_all_aliases() {
        let h = TestHarness::new().await;
        {
            let cfg_lock = h.factory.config().unwrap();
            let mut cfg = cfg_lock.lock().unwrap();
            cfg.set_alias("co", "pr checkout");
            cfg.set_alias("iv", "issue view");
        }

        let args = DeleteArgs {
            name: None,
            all: true,
        };
        args.run(&h.factory).unwrap();

        let cfg_lock = h.factory.config().unwrap();
        let cfg = cfg_lock.lock().unwrap();
        assert!(cfg.aliases().is_empty());
    }

    #[tokio::test]
    async fn test_should_error_delete_all_when_no_aliases() {
        let h = TestHarness::new().await;
        let args = DeleteArgs {
            name: None,
            all: true,
        };
        let result = args.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no aliases"));
    }
}
