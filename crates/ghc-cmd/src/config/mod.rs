//! Configuration commands (`ghc config`).
//!
//! Maps from Go's `pkg/cmd/config/` package. Provides get, set, list,
//! and clear-cache subcommands for managing GHC configuration.

pub mod clear_cache;
pub mod get;
pub mod list;
pub mod set;

use clap::Subcommand;

use crate::factory::Factory;

/// Config subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the value of a given configuration key.
    Get(get::GetArgs),
    /// Update configuration with a value for the given key.
    Set(set::SetArgs),
    /// Print a list of configuration keys and values.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Clear the CLI cache.
    ClearCache(clear_cache::ClearCacheArgs),
}

impl ConfigCommand {
    /// Run the appropriate config subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub fn run(self, factory: &Factory) -> anyhow::Result<()> {
        match self {
            Self::Get(args) => args.run(factory),
            Self::Set(args) => args.run(factory),
            Self::List(args) => args.run(factory),
            Self::ClearCache(args) => args.run(factory),
        }
    }
}
