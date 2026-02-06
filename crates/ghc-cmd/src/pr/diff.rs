//! `ghc pr diff` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_print, ios_println};

/// View the diff of a pull request.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct DiffArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Use colored diff output.
    #[arg(long)]
    color: bool,

    /// Name-only: show only names of changed files.
    #[arg(long)]
    name_only: bool,

    /// Display the raw patch format instead of unified diff.
    #[arg(long)]
    patch: bool,

    /// Open the diff in the web browser.
    #[arg(short, long)]
    web: bool,
}

impl DiffArgs {
    /// Run the pr diff command.
    ///
    /// Fetches the diff of the pull request from the GitHub API using the
    /// diff media type endpoint. The diff is retrieved as plain text via
    /// `rest_text` and printed to stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/pull/{}/files",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.number,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        // Fetch the diff using the REST text endpoint.
        let path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );

        // Get the diff via the PR files endpoint for name-only mode
        if self.name_only {
            let files_path = format!("{path}/files");
            let files: serde_json::Value = client
                .rest(
                    reqwest::Method::GET,
                    &files_path,
                    None::<&serde_json::Value>,
                )
                .await
                .context("failed to fetch pull request files")?;

            let file_list = files
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("unexpected API response format"))?;

            for file in file_list {
                let filename = file.get("filename").and_then(Value::as_str).unwrap_or("");
                ios_println!(ios, "{filename}");
            }
            return Ok(());
        }

        // Fetch the full diff content. The GitHub REST API at
        // /repos/{owner}/{repo}/pulls/{number} returns the diff when the
        // Accept header is set to application/vnd.github.v3.diff. Since our
        // client does not support custom headers on REST calls, we use the
        // web URL pattern which provides the diff as plain text.
        let suffix = if self.patch { "patch" } else { "diff" };
        let diff_url = format!(
            "https://{}/{}/{}/pull/{}.{suffix}",
            repo.host(),
            repo.owner(),
            repo.name(),
            self.number,
        );

        let diff_text = client
            .rest_text(reqwest::Method::GET, &diff_url, None)
            .await
            .context("failed to fetch pull request diff")?;

        if self.color {
            // Basic colorization of unified diff output
            let cs = ios.color_scheme();
            for line in diff_text.lines() {
                if line.starts_with('+') && !line.starts_with("+++") {
                    ios_println!(ios, "{}", cs.success(line));
                } else if line.starts_with('-') && !line.starts_with("---") {
                    ios_println!(ios, "{}", cs.error(line));
                } else if line.starts_with("@@") {
                    ios_println!(ios, "{}", cs.cyan(line));
                } else {
                    ios_println!(ios, "{line}");
                }
            }
        } else {
            ios_print!(ios, "{diff_text}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_changed_file_names() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/40/files",
            serde_json::json!([
                { "filename": "src/main.rs", "status": "modified" },
                { "filename": "README.md", "status": "modified" }
            ]),
        )
        .await;

        let args = DiffArgs {
            number: 40,
            repo: "owner/repo".into(),
            color: false,
            name_only: true,
            patch: false,
            web: false,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("src/main.rs"),
            "should contain filename: {out}"
        );
        assert!(out.contains("README.md"), "should contain filename: {out}");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let args = DiffArgs {
            number: 40,
            repo: "owner/repo".into(),
            color: false,
            name_only: false,
            patch: false,
            web: true,
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("/pull/40/files"),
            "should open diff URL: {}",
            urls[0]
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_diff() {
        let h = TestHarness::new().await;
        let args = DiffArgs {
            number: 1,
            repo: "bad".into(),
            color: false,
            name_only: false,
            patch: false,
            web: false,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
