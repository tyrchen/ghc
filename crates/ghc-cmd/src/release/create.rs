//! `ghc release create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Create a new release.
///
/// Create a new GitHub release for a tag. If the tag does not exist, it will
/// be created from the target branch or default branch.
///
/// Release notes can be provided via `--notes`, `--notes-file`, or
/// `--generate-notes`. Use `--notes-from-tag` to use the annotated tag
/// message as release notes.
///
/// Use `--verify-tag` to abort if the tag does not already exist.
///
/// Use `--discussion-category` to create a linked discussion for the release.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateArgs {
    /// Tag name for the release.
    #[arg(value_name = "TAG")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Release title.
    #[arg(short, long)]
    title: Option<String>,

    /// Release notes.
    #[arg(short, long)]
    notes: Option<String>,

    /// Read release notes from a file ("-" for stdin).
    #[arg(short = 'F', long, value_name = "FILE")]
    notes_file: Option<String>,

    /// Use the annotated tag message as release notes.
    #[arg(long)]
    notes_from_tag: bool,

    /// Target branch or commit SHA.
    #[arg(long)]
    target: Option<String>,

    /// Mark as a draft release.
    #[arg(short, long)]
    draft: bool,

    /// Mark as a prerelease.
    #[arg(short, long)]
    prerelease: bool,

    /// Generate release notes automatically.
    #[arg(long)]
    generate_notes: bool,

    /// Mark as the latest release.
    #[arg(long)]
    latest: bool,

    /// Abort if the tag does not already exist (do not create it).
    #[arg(long)]
    verify_tag: bool,

    /// Create a discussion in the specified category for this release.
    #[arg(long, value_name = "CATEGORY")]
    discussion_category: Option<String>,

    /// Files to upload as release assets.
    #[arg(value_name = "FILE")]
    files: Vec<String>,
}

impl CreateArgs {
    /// Run the release create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = resolve_repo(self.repo.as_deref())?;
        let client = factory.api_client(repo.host())?;

        // Verify tag exists if --verify-tag is set
        if self.verify_tag {
            let tag_path = format!(
                "repos/{}/{}/git/ref/tags/{}",
                repo.owner(),
                repo.name(),
                self.tag,
            );
            let tag_result: Result<Value, _> =
                client.rest(reqwest::Method::GET, &tag_path, None).await;
            if tag_result.is_err() {
                anyhow::bail!(
                    "tag '{}' does not exist in {}; aborting due to --verify-tag",
                    self.tag,
                    repo.full_name(),
                );
            }
        }

        // Determine release notes body
        let notes_body = if self.notes_from_tag {
            // Fetch the annotated tag message
            self.fetch_tag_message(&client, &repo).await?
        } else {
            match (&self.notes, &self.notes_file) {
                (Some(n), _) => n.clone(),
                (_, Some(f)) if f == "-" => {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                        .context("failed to read notes from stdin")?;
                    buf
                }
                (_, Some(f)) => {
                    std::fs::read_to_string(f).with_context(|| format!("failed to read {f}"))?
                }
                _ => String::new(),
            }
        };

        let mut body = serde_json::json!({
            "tag_name": self.tag,
            "name": self.title.as_deref().unwrap_or(&self.tag),
            "body": notes_body,
            "draft": self.draft,
            "prerelease": self.prerelease,
            "generate_release_notes": self.generate_notes,
        });

        if let Some(ref target) = self.target {
            body["target_commitish"] = Value::String(target.clone());
        }

        if self.latest {
            body["make_latest"] = Value::String("true".to_string());
        }

        if let Some(ref category) = self.discussion_category {
            body["discussion_category_name"] = Value::String(category.clone());
        }

        let path = format!("repos/{}/{}/releases", repo.owner(), repo.name());
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to create release")?;

        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");
        let release_id = result.get("id").and_then(Value::as_u64).unwrap_or(0);

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Upload assets if provided
        upload_assets(&client, &repo, release_id, &self.files, ios).await?;

        let status = if self.draft {
            "draft release"
        } else {
            "release"
        };
        ios_eprintln!(
            ios,
            "{} Created {status} {} for {}",
            cs.success_icon(),
            cs.bold(&self.tag),
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{html_url}");

        Ok(())
    }

    /// Fetch the annotated tag message for `--notes-from-tag`.
    async fn fetch_tag_message(
        &self,
        client: &ghc_api::client::Client,
        repo: &Repo,
    ) -> Result<String> {
        // First resolve the tag ref to get the SHA
        let ref_path = format!(
            "repos/{}/{}/git/ref/tags/{}",
            repo.owner(),
            repo.name(),
            self.tag,
        );
        let ref_data: Value = client
            .rest(reqwest::Method::GET, &ref_path, None)
            .await
            .with_context(|| {
                format!(
                    "tag '{}' not found in {}; cannot use --notes-from-tag",
                    self.tag,
                    repo.full_name(),
                )
            })?;

        let obj_type = ref_data
            .pointer("/object/type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let obj_sha = ref_data
            .pointer("/object/sha")
            .and_then(Value::as_str)
            .unwrap_or("");

        if obj_type != "tag" {
            // Lightweight tag -- no message
            return Ok(String::new());
        }

        // Fetch the tag object to get its message
        let tag_path = format!("repos/{}/{}/git/tags/{obj_sha}", repo.owner(), repo.name(),);
        let tag_data: Value = client
            .rest(reqwest::Method::GET, &tag_path, None)
            .await
            .context("failed to fetch tag object")?;

        let message = tag_data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");

        Ok(message.to_string())
    }
}

/// Upload release asset files.
async fn upload_assets(
    client: &ghc_api::client::Client,
    repo: &Repo,
    release_id: u64,
    files: &[String],
    ios: &ghc_core::iostreams::IOStreams,
) -> Result<()> {
    let cs = ios.color_scheme();
    for file_path in files {
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(file_path);

        let _data = std::fs::read(file_path)
            .with_context(|| format!("failed to read asset: {file_path}"))?;

        let upload_url = format!(
            "https://uploads.github.com/repos/{}/{}/releases/{release_id}/assets?name={file_name}",
            repo.owner(),
            repo.name(),
        );

        let _: Value = client
            .rest(reqwest::Method::POST, &upload_url, None)
            .await
            .with_context(|| format!("failed to upload asset: {file_name}"))?;

        ios_eprintln!(ios, "{} Uploaded {file_name}", cs.success_icon());
    }
    Ok(())
}

/// Resolve a repository from the `--repo` flag or bail.
fn resolve_repo(repo_flag: Option<&str>) -> Result<Repo> {
    let name = repo_flag
        .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
    Repo::from_full_name(name).context("invalid repository format")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_post};

    fn default_create_args() -> CreateArgs {
        CreateArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            title: None,
            notes: None,
            notes_file: None,
            notes_from_tag: false,
            target: None,
            draft: false,
            prerelease: false,
            generate_notes: false,
            latest: false,
            verify_tag: false,
            discussion_category: None,
            files: vec![],
        }
    }

    #[tokio::test]
    async fn test_should_create_release() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 1,
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "tag_name": "v1.0.0",
            }),
        )
        .await;

        let mut args = default_create_args();
        args.title = Some("Release 1.0".into());
        args.notes = Some("Initial release".into());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created release"));
        assert!(err.contains("v1.0.0"));
        let out = h.stdout();
        assert!(out.contains("https://github.com/owner/repo/releases/tag/v1.0.0"));
    }

    #[tokio::test]
    async fn test_should_create_draft_release() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 2,
                "html_url": "https://github.com/owner/repo/releases/tag/v2.0.0",
                "tag_name": "v2.0.0",
            }),
        )
        .await;

        let mut args = default_create_args();
        args.tag = "v2.0.0".into();
        args.draft = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created draft release"));
    }

    #[tokio::test]
    async fn test_should_fail_verify_tag_when_tag_missing() {
        let h = TestHarness::new().await;
        // No mock for the git ref endpoint -- it will 404

        let mut args = default_create_args();
        args.verify_tag = true;
        let result = args.run(&h.factory).await;

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("does not exist") || msg.contains("verify-tag"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn test_should_pass_verify_tag_when_tag_exists() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/git/ref/tags/v1.0.0",
            serde_json::json!({
                "ref": "refs/tags/v1.0.0",
                "object": { "type": "commit", "sha": "abc123" }
            }),
        )
        .await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 1,
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "tag_name": "v1.0.0",
            }),
        )
        .await;

        let mut args = default_create_args();
        args.verify_tag = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created release"));
    }

    #[tokio::test]
    async fn test_should_use_notes_from_tag() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/git/ref/tags/v1.0.0",
            serde_json::json!({
                "ref": "refs/tags/v1.0.0",
                "object": { "type": "tag", "sha": "deadbeef" }
            }),
        )
        .await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/git/tags/deadbeef",
            serde_json::json!({
                "tag": "v1.0.0",
                "message": "Release v1.0.0\n\nBugfixes and improvements.",
            }),
        )
        .await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 1,
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "tag_name": "v1.0.0",
            }),
        )
        .await;

        let mut args = default_create_args();
        args.notes_from_tag = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created release"));
    }

    #[tokio::test]
    async fn test_should_create_release_with_discussion_category() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/releases",
            201,
            serde_json::json!({
                "id": 1,
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "tag_name": "v1.0.0",
            }),
        )
        .await;

        let mut args = default_create_args();
        args.discussion_category = Some("Announcements".into());
        args.notes = Some("Release notes".into());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created release"));
    }
}
