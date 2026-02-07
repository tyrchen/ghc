//! `ghc release view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// View a release.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Tag name of the release (or "latest").
    #[arg(value_name = "TAG", default_value = "latest")]
    tag: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Open the release in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ViewArgs {
    /// Run the release view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release cannot be viewed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;

        if self.web {
            let url = if self.tag == "latest" {
                format!(
                    "https://{}/{}/{}/releases/latest",
                    repo.host(),
                    repo.owner(),
                    repo.name(),
                )
            } else {
                format!(
                    "https://{}/{}/{}/releases/tag/{}",
                    repo.host(),
                    repo.owner(),
                    repo.name(),
                    self.tag,
                )
            };
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let path = if self.tag == "latest" {
            format!("repos/{}/{}/releases/latest", repo.owner(), repo.name(),)
        } else {
            format!(
                "repos/{}/{}/releases/tags/{}",
                repo.owner(),
                repo.name(),
                self.tag,
            )
        };

        let mut release: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch release")?;

        // Normalize REST field names to match gh CLI conventions
        // (gh uses isDraft/isPrerelease/tagName/publishedAt/createdAt)
        super::normalize_release_fields(&mut release);

        // For single release view, compute isLatest:
        // - If fetched via "latest" endpoint, it is the latest
        // - Otherwise, check draft/prerelease status (non-draft, non-prerelease = potentially latest)
        if let Some(obj) = release.as_object_mut() {
            let is_draft = obj.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
            let is_pre = obj
                .get("isPrerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_latest = self.tag == "latest" || (!is_draft && !is_pre);
            obj.insert("isLatest".to_string(), Value::Bool(is_latest));
        }

        // JSON output
        let ios = &factory.io;
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &release,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let title = release.get("name").and_then(Value::as_str).unwrap_or("");
        let tag_name = release
            .get("tagName")
            .or_else(|| release.get("tag_name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let body = release.get("body").and_then(Value::as_str).unwrap_or("");
        let is_draft = release
            .get("isDraft")
            .or_else(|| release.get("draft"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_prerelease = release
            .get("isPrerelease")
            .or_else(|| release.get("prerelease"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let published_at = release
            .get("publishedAt")
            .or_else(|| release.get("published_at"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let created_at = release
            .get("createdAt")
            .or_else(|| release.get("created_at"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let html_url = release
            .get("htmlUrl")
            .or_else(|| release.get("html_url"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let author = release
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("");

        let is_immutable = release
            .get("immutable")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let assets = release
            .get("assets")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Key-value output (matches gh CLI format)
        ios_println!(ios, "title:\t{title}");
        ios_println!(ios, "tag:\t{tag_name}");
        ios_println!(ios, "draft:\t{is_draft}");
        ios_println!(ios, "prerelease:\t{is_prerelease}");
        ios_println!(ios, "author:\t{author}");
        ios_println!(ios, "created:\t{created_at}");
        ios_println!(ios, "published:\t{published_at}");
        ios_println!(ios, "immutable:\t{is_immutable}");
        ios_println!(ios, "url:\t{html_url}");

        if !assets.is_empty() {
            ios_println!(ios, "assets:");
            for asset in &assets {
                let name = asset
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let size = asset.get("size").and_then(Value::as_u64).unwrap_or(0);
                ios_println!(ios, "  - {name} ({size} bytes)");
            }
        }

        ios_println!(ios, "--");
        if body.is_empty() {
            ios_println!(ios, "No release notes.");
        } else {
            ios_println!(ios, "{body}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_view_release() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases/tags/v1.0.0",
            serde_json::json!({
                "tag_name": "v1.0.0",
                "name": "Release 1.0",
                "body": "Release notes here",
                "draft": false,
                "prerelease": false,
                "published_at": "2024-01-15T10:00:00Z",
                "html_url": "https://github.com/owner/repo/releases/tag/v1.0.0",
                "assets": [
                    {"name": "binary.tar.gz", "size": 1024}
                ]
            }),
        )
        .await;

        let args = ViewArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            web: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("title:\tRelease 1.0"),
            "should contain key-value title: {out}"
        );
        assert!(
            out.contains("tag:\tv1.0.0"),
            "should contain key-value tag: {out}"
        );
        assert!(
            out.contains("draft:\tfalse"),
            "should contain draft status: {out}"
        );
        assert!(
            out.contains("prerelease:\tfalse"),
            "should contain prerelease status: {out}"
        );
        assert!(out.contains("Release notes here"));
        assert!(out.contains("binary.tar.gz"));
    }

    #[tokio::test]
    async fn test_should_view_release_in_browser() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            tag: "v1.0.0".into(),
            repo: Some("owner/repo".into()),
            web: true,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("releases/tag/v1.0.0"));
    }

    #[tokio::test]
    async fn test_should_view_latest_release_in_browser() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            tag: "latest".into(),
            repo: Some("owner/repo".into()),
            web: true,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("releases/latest"));
    }
}
