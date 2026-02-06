//! `ghc release list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List releases.
///
/// Lists releases for a repository. By default displays as a table.
/// Use `--json` to output JSON with specified fields.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Maximum number of releases to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Exclude draft releases.
    #[arg(long)]
    exclude_drafts: bool,

    /// Exclude prerelease releases.
    #[arg(long)]
    exclude_pre_releases: bool,

    /// Order by creation date (default: most recent first).
    #[arg(long, value_parser = ["asc", "desc"], default_value = "desc")]
    order: String,

    /// Output JSON with specified fields (e.g., "tagName,name,isDraft,isPrerelease").
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ListArgs {
    /// Run the release list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the releases cannot be listed.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let path = format!(
            "repos/{}/{}/releases?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit.min(100),
        );

        let releases: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list releases")?;

        // JSON output mode - always produces output (even [] for empty results)
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let filtered: Vec<Value> = releases
                .iter()
                .filter(|r| {
                    let is_draft = r.get("draft").and_then(Value::as_bool).unwrap_or(false);
                    let is_pre = r
                        .get("prerelease")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if self.exclude_drafts && is_draft {
                        return false;
                    }
                    if self.exclude_pre_releases && is_pre {
                        return false;
                    }
                    true
                })
                .cloned()
                .map(|mut r| {
                    super::normalize_release_fields(&mut r);
                    r
                })
                .collect();
            let mut arr = Value::Array(filtered);
            super::compute_is_latest(&mut arr);
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if releases.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No releases found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        // Track whether we've seen the "Latest" release
        // Only the first non-draft, non-prerelease is "Latest"
        let mut found_latest = false;

        for release in &releases {
            let tag = release
                .get("tag_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let title = release.get("name").and_then(Value::as_str).unwrap_or(tag);
            let is_draft = release
                .get("draft")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_prerelease = release
                .get("prerelease")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let published_at = release
                .get("published_at")
                .and_then(Value::as_str)
                .unwrap_or("");

            if self.exclude_drafts && is_draft {
                continue;
            }
            if self.exclude_pre_releases && is_prerelease {
                continue;
            }

            let status = if is_draft {
                cs.warning("Draft")
            } else if is_prerelease {
                cs.cyan("Pre-release")
            } else if !found_latest {
                found_latest = true;
                cs.success("Latest")
            } else {
                String::new()
            };

            tp.add_row(vec![
                cs.bold(title),
                status,
                tag.to_string(),
                published_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_releases() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases",
            serde_json::json!([
                {
                    "tag_name": "v1.0.0",
                    "name": "Release 1.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-01-15T10:00:00Z"
                },
                {
                    "tag_name": "v0.9.0",
                    "name": "Beta Release",
                    "draft": false,
                    "prerelease": true,
                    "published_at": "2024-01-10T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            exclude_drafts: false,
            exclude_pre_releases: false,
            order: "desc".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("Release 1.0"));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("Beta Release"));
    }

    #[tokio::test]
    async fn test_should_list_releases_excluding_prereleases() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases",
            serde_json::json!([
                {
                    "tag_name": "v1.0.0",
                    "name": "Release 1.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-01-15T10:00:00Z"
                },
                {
                    "tag_name": "v0.9.0-rc1",
                    "name": "RC1",
                    "draft": false,
                    "prerelease": true,
                    "published_at": "2024-01-10T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            exclude_drafts: false,
            exclude_pre_releases: true,
            order: "desc".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("Release 1.0"));
        assert!(!out.contains("RC1"));
    }

    #[tokio::test]
    async fn test_should_only_mark_first_release_as_latest() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases",
            serde_json::json!([
                {
                    "tag_name": "v2.0.0",
                    "name": "Release 2.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-02-15T10:00:00Z"
                },
                {
                    "tag_name": "v1.0.0",
                    "name": "Release 1.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-01-15T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            exclude_drafts: false,
            exclude_pre_releases: false,
            order: "desc".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("Latest"), "should have Latest tag");
        // Count occurrences of "Latest" - should be exactly 1
        let latest_count = out.matches("Latest").count();
        assert_eq!(
            latest_count, 1,
            "only one release should be marked Latest, found {latest_count}"
        );
    }

    #[tokio::test]
    async fn test_should_fail_without_repo_flag() {
        let h = TestHarness::new().await;

        let args = ListArgs {
            repo: None,
            limit: 30,
            exclude_drafts: false,
            exclude_pre_releases: false,
            order: "desc".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_should_output_json_when_json_flag_set() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases",
            serde_json::json!([
                {
                    "tag_name": "v1.0.0",
                    "name": "Release 1.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-01-15T10:00:00Z"
                },
                {
                    "tag_name": "v0.9.0",
                    "name": "Beta",
                    "draft": true,
                    "prerelease": false,
                    "published_at": "2024-01-10T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            exclude_drafts: false,
            exclude_pre_releases: false,
            order: "desc".into(),
            json: vec!["tag_name".into()],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        // JSON output should contain the filtered release data
        assert!(out.contains("\"tag_name\""));
        assert!(out.contains("v1.0.0"));
        assert!(out.contains("v0.9.0"));
    }

    #[tokio::test]
    async fn test_should_output_json_with_exclude_filter() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/releases",
            serde_json::json!([
                {
                    "tag_name": "v1.0.0",
                    "name": "Release 1.0",
                    "draft": false,
                    "prerelease": false,
                    "published_at": "2024-01-15T10:00:00Z"
                },
                {
                    "tag_name": "v0.9.0",
                    "name": "Draft",
                    "draft": true,
                    "prerelease": false,
                    "published_at": "2024-01-10T10:00:00Z"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".into()),
            limit: 30,
            exclude_drafts: true,
            exclude_pre_releases: false,
            order: "desc".into(),
            json: vec!["tag_name".into()],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("v1.0.0"));
        assert!(
            !out.contains("v0.9.0"),
            "draft should be excluded from JSON output"
        );
    }
}
