//! Alias commands (`ghc alias`).
//!
//! Maps from Go's `pkg/cmd/alias/` package. Provides set, list, delete,
//! and import subcommands for managing command shortcuts.

pub mod delete;
pub mod imports;
pub mod list;
pub mod set;

use clap::Subcommand;

use crate::factory::Factory;

/// Alias subcommands.
#[derive(Debug, Subcommand)]
pub enum AliasCommand {
    /// Create a shortcut for a ghc command.
    Set(set::SetArgs),
    /// List your aliases.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Delete set aliases.
    Delete(delete::DeleteArgs),
    /// Import aliases from a YAML file.
    Import(imports::ImportArgs),
}

impl AliasCommand {
    /// Run the appropriate alias subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub fn run(self, factory: &Factory) -> anyhow::Result<()> {
        match self {
            Self::Set(args) => args.run(factory),
            Self::List(args) => args.run(factory),
            Self::Delete(args) => args.run(factory),
            Self::Import(args) => args.run(factory),
        }
    }
}
