//! Release commands (`ghc release`).
//!
//! Manage releases for a GitHub repository.

pub mod create;
pub mod delete;
pub mod delete_asset;
pub mod download;
pub mod edit;
pub mod list;
pub mod upload;
pub mod verify;
pub mod verify_asset;
pub mod view;

use clap::Subcommand;

/// Manage releases.
#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    /// Create a new release.
    Create(create::CreateArgs),
    /// Delete a release.
    Delete(delete::DeleteArgs),
    /// Delete an asset from a release.
    #[command(name = "delete-asset")]
    DeleteAsset(delete_asset::DeleteAssetArgs),
    /// Download release assets.
    Download(download::DownloadArgs),
    /// Edit a release.
    Edit(edit::EditArgs),
    /// List releases.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Upload assets to a release.
    Upload(upload::UploadArgs),
    /// Verify the attestation for a release.
    Verify(verify::VerifyArgs),
    /// Verify that an asset originated from a release.
    #[command(name = "verify-asset")]
    VerifyAsset(verify_asset::VerifyAssetArgs),
    /// View a release.
    View(view::ViewArgs),
}

impl ReleaseCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::DeleteAsset(args) => args.run(factory).await,
            Self::Download(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Upload(args) => args.run(factory).await,
            Self::Verify(args) => args.run(factory).await,
            Self::VerifyAsset(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
