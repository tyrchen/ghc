//! `ghc alias set` command.

use std::io::Read;

use anyhow::Result;
use clap::Args;

use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Create a shortcut for a ghc command.
#[derive(Debug, Args)]
pub struct SetArgs {
    /// The alias name.
    name: String,
    /// The expansion (command the alias maps to).
    expansion: String,
    /// Declare an alias to be passed through a shell interpreter.
    #[arg(short, long)]
    shell: bool,
    /// Overwrite existing aliases of the same name.
    #[arg(long)]
    clobber: bool,
}

impl SetArgs {
    /// Run the alias set command.
    ///
    /// # Errors
    ///
    /// Returns an error if the alias cannot be created.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let mut expansion = if self.expansion == "-" {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            self.expansion.clone()
        };

        if self.shell && !expansion.starts_with('!') {
            expansion = format!("!{expansion}");
        }

        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let existing = cfg.aliases().contains_key(&self.name);

        if existing && !self.clobber {
            anyhow::bail!(
                "could not create alias {}: name already taken, use the --clobber flag to overwrite it",
                self.name
            );
        }

        if ios.is_stdout_tty() {
            ios_eprintln!(ios, "- Creating alias for {}: {}", self.name, expansion);
        }

        cfg.set_alias(&self.name, &expansion);
        cfg.write()?;

        if ios.is_stdout_tty() {
            let cs = ios.color_scheme();
            if existing && self.clobber {
                ios_eprintln!(ios, "{} Changed alias {}", cs.warning_icon(), self.name);
            } else {
                ios_eprintln!(ios, "{} Added alias {}", cs.success_icon(), self.name);
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
    async fn test_should_set_alias() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            name: "co".to_string(),
            expansion: "pr checkout".to_string(),
            shell: false,
            clobber: false,
        };
        args.run(&h.factory).unwrap();

        // Verify alias was set in config
        let cfg_lock = h.factory.config().unwrap();
        let cfg = cfg_lock.lock().unwrap();
        assert_eq!(cfg.aliases().get("co"), Some(&"pr checkout".to_string()));
    }

    #[tokio::test]
    async fn test_should_error_on_duplicate_without_clobber() {
        let h = TestHarness::new().await;
        // Set alias first
        let args = SetArgs {
            name: "co".to_string(),
            expansion: "pr checkout".to_string(),
            shell: false,
            clobber: false,
        };
        args.run(&h.factory).unwrap();

        // Try to set again without clobber
        let args2 = SetArgs {
            name: "co".to_string(),
            expansion: "pr list".to_string(),
            shell: false,
            clobber: false,
        };
        let result = args2.run(&h.factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already taken"));
    }

    #[tokio::test]
    async fn test_should_overwrite_with_clobber() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            name: "co".to_string(),
            expansion: "pr checkout".to_string(),
            shell: false,
            clobber: false,
        };
        args.run(&h.factory).unwrap();

        let args2 = SetArgs {
            name: "co".to_string(),
            expansion: "pr list".to_string(),
            shell: false,
            clobber: true,
        };
        args2.run(&h.factory).unwrap();

        let cfg_lock = h.factory.config().unwrap();
        let cfg = cfg_lock.lock().unwrap();
        assert_eq!(cfg.aliases().get("co"), Some(&"pr list".to_string()));
    }

    #[tokio::test]
    async fn test_should_add_shell_prefix() {
        let h = TestHarness::new().await;
        let args = SetArgs {
            name: "greet".to_string(),
            expansion: "echo hello".to_string(),
            shell: true,
            clobber: false,
        };
        args.run(&h.factory).unwrap();

        let cfg_lock = h.factory.config().unwrap();
        let cfg = cfg_lock.lock().unwrap();
        assert_eq!(cfg.aliases().get("greet"), Some(&"!echo hello".to_string()));
    }
}
