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
use serde_json::Value;

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

/// Normalize REST API release field names to match gh CLI conventions.
///
/// Maps: `draft` -> `isDraft`, `prerelease` -> `isPrerelease`,
/// `tag_name` -> `tagName`, `published_at` -> `publishedAt`, etc.
fn normalize_release_fields(release: &mut Value) {
    if let Some(obj) = release.as_object_mut() {
        let mappings: &[(&str, &str)] = &[
            ("draft", "isDraft"),
            ("prerelease", "isPrerelease"),
            ("tag_name", "tagName"),
            ("published_at", "publishedAt"),
            ("html_url", "htmlUrl"),
            ("created_at", "createdAt"),
            ("target_commitish", "targetCommitish"),
            ("upload_url", "uploadUrl"),
            ("tarball_url", "tarballUrl"),
            ("zipball_url", "zipballUrl"),
        ];
        for &(rest_name, gh_name) in mappings {
            if let Some(val) = obj.get(rest_name).cloned() {
                obj.insert(gh_name.to_string(), val);
            }
        }
    }
}

/// Compute and insert the `isLatest` field for each release in a JSON array.
///
/// The first non-draft, non-prerelease release is marked `isLatest: true`;
/// all others get `isLatest: false`. This matches `gh release list --json isLatest`.
fn compute_is_latest(releases: &mut Value) {
    if let Some(arr) = releases.as_array_mut() {
        let mut found_latest = false;
        for release in arr.iter_mut() {
            if let Some(obj) = release.as_object_mut() {
                let is_draft = obj
                    .get("isDraft")
                    .or_else(|| obj.get("draft"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let is_pre = obj
                    .get("isPrerelease")
                    .or_else(|| obj.get("prerelease"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let is_latest = !found_latest && !is_draft && !is_pre;
                if is_latest {
                    found_latest = true;
                }
                obj.insert("isLatest".to_string(), Value::Bool(is_latest));
            }
        }
    }
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
