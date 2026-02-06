//! `ghc alias import` command.

use std::collections::BTreeMap;
use std::io::Read;

use anyhow::Result;
use clap::Args;

use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Import aliases from a YAML file.
#[derive(Debug, Args)]
pub struct ImportArgs {
    /// File to import aliases from, or `-` for stdin.
    #[arg(default_value = "-")]
    filename: String,
    /// Overwrite existing aliases of the same name.
    #[arg(long)]
    clobber: bool,
}

impl ImportArgs {
    /// Run the alias import command.
    ///
    /// # Errors
    ///
    /// Returns an error if the aliases cannot be imported.
    pub fn run(&self, factory: &Factory) -> Result<()> {
        let ios = &factory.io;
        let content = if self.filename == "-" {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        } else {
            std::fs::read_to_string(&self.filename)?
        };

        let alias_map: BTreeMap<String, String> = serde_yaml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse YAML: {e}"))?;

        if ios.is_stdout_tty() {
            if self.filename == "-" {
                ios_eprintln!(ios, "- Importing aliases from standard input");
            } else {
                ios_eprintln!(ios, "- Importing aliases from file {:?}", self.filename);
            }
        }

        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let cs = ios.color_scheme();

        for (alias, expansion) in &alias_map {
            let existing = cfg.aliases().contains_key(alias);

            if existing && !self.clobber {
                if ios.is_stdout_tty() {
                    ios_eprintln!(
                        ios,
                        "{} Could not import alias {}: name already taken",
                        cs.error_icon(),
                        alias,
                    );
                }
                continue;
            }

            cfg.set_alias(alias, expansion);

            if ios.is_stdout_tty() {
                if existing && self.clobber {
                    ios_eprintln!(ios, "{} Changed alias {}", cs.warning_icon(), alias);
                } else {
                    ios_eprintln!(ios, "{} Added alias {}", cs.success_icon(), alias);
                }
            }
        }

        cfg.write()?;

        Ok(())
    }
}
