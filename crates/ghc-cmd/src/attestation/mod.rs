//! Attestation commands (`ghc attestation`).
//!
//! Work with artifact attestations.

pub mod verify;

use clap::Subcommand;

/// Work with artifact attestations.
#[derive(Debug, Subcommand)]
pub enum AttestationCommand {
    /// Verify an artifact attestation.
    Verify(verify::VerifyArgs),
}

impl AttestationCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Verify(args) => args.run(factory).await,
        }
    }
}
