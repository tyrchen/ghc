//! Attestation commands (`ghc attestation`).
//!
//! Work with artifact attestations.

pub mod download;
pub mod inspect;
pub mod trusted_root;
pub mod verify;

use clap::Subcommand;

/// Work with artifact attestations.
#[derive(Debug, Subcommand)]
pub enum AttestationCommand {
    /// Download an artifact's attestations for offline use.
    Download(download::DownloadArgs),
    /// Inspect a Sigstore bundle.
    Inspect(inspect::InspectArgs),
    /// Output trusted_root.jsonl contents for offline verification.
    #[command(name = "trusted-root")]
    TrustedRoot(trusted_root::TrustedRootArgs),
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
            Self::Download(args) => args.run(factory).await,
            Self::Inspect(args) => args.run(factory),
            Self::TrustedRoot(args) => args.run(factory).await,
            Self::Verify(args) => args.run(factory).await,
        }
    }
}
