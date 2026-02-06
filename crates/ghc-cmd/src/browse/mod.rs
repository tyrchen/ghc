//! Browse command (`ghc browse`).
//!
//! Open a repository in the web browser. Supports opening specific files
//! with line number ranges (e.g., `main.go:10-20`), issues/PRs by number,
//! and commits by SHA.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Open a GitHub repository in the web browser.
///
/// A browser location can be specified using arguments in the following format:
/// - by number for issue or pull request, e.g. "123"
/// - by path for opening folders and files, e.g. "cmd/gh/main.go"
/// - by path with line range, e.g. "main.go:10" or "main.go:10-20"
/// - by commit SHA
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct BrowseArgs {
    /// The file, path, issue/PR number, or commit SHA to browse.
    #[arg(value_name = "LOCATION")]
    location: Option<String>,

    /// Repository to browse (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Open the repository's settings page.
    #[arg(short, long)]
    settings: bool,

    /// Open the repository's wiki.
    #[arg(short, long)]
    wiki: bool,

    /// Open the repository's issues.
    #[arg(short, long)]
    issues: bool,

    /// Open the repository's pull requests.
    #[arg(short, long)]
    pulls: bool,

    /// Open the repository's releases.
    #[arg(short = 'r', long)]
    releases: bool,

    /// Open the repository's projects.
    #[arg(long)]
    projects: bool,

    /// Open the repository's actions.
    #[arg(short, long)]
    actions: bool,

    /// Branch name.
    #[arg(short, long)]
    branch: Option<String>,

    /// Print the URL instead of opening it.
    #[arg(short = 'n', long)]
    no_browser: bool,

    /// Select a specific commit.
    #[arg(short, long)]
    commit: Option<String>,
}

impl BrowseArgs {
    /// Run the browse command.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be determined or browser cannot be opened.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo_str = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository required (use -R OWNER/REPO)"))?;

        let repo =
            ghc_core::repo::Repo::from_full_name(repo_str).context("invalid repository format")?;

        let base_url = format!("https://{}/{}/{}", repo.host(), repo.owner(), repo.name());

        // Fetch the default branch if we need it (file location without explicit branch)
        let default_branch =
            if self.location.is_some() && self.branch.is_none() && self.commit.is_none() {
                self.fetch_default_branch(factory, &repo).await.ok()
            } else {
                None
            };

        let section = self.parse_section(default_branch.as_deref())?;
        let url = if section.is_empty() {
            base_url
        } else {
            format!("{base_url}/{section}")
        };

        let ios = &factory.io;
        if self.no_browser {
            ios_println!(ios, "{url}");
        } else {
            factory.browser().open(&url)?;
            let cs = ios.color_scheme();
            ios_eprintln!(ios, "{} Opening {} in your browser", cs.success_icon(), url);
        }

        Ok(())
    }

    /// Fetch the repository's default branch name.
    async fn fetch_default_branch(
        &self,
        factory: &crate::factory::Factory,
        repo: &ghc_core::repo::Repo,
    ) -> Result<String> {
        let client = factory.api_client(repo.host())?;
        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));

        let data: Value = client
            .graphql(
                "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { defaultBranchRef { name } } }",
                &variables,
            )
            .await
            .context("failed to fetch default branch")?;

        data.pointer("/repository/defaultBranchRef/name")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("could not determine default branch"))
    }

    /// Parse the URL section based on flags and location argument.
    fn parse_section(&self, default_branch: Option<&str>) -> Result<String> {
        // Section flags take priority
        if self.settings {
            return Ok("settings".to_string());
        }
        if self.wiki {
            return Ok("wiki".to_string());
        }
        if self.issues {
            return Ok("issues".to_string());
        }
        if self.pulls {
            return Ok("pulls".to_string());
        }
        if self.releases {
            return Ok("releases".to_string());
        }
        if self.projects {
            return Ok("projects".to_string());
        }
        if self.actions {
            return Ok("actions".to_string());
        }

        // Determine ref (branch or commit)
        let git_ref = if let Some(ref commit) = self.commit {
            Some(commit.clone())
        } else {
            self.branch.clone()
        };

        let location = if let Some(loc) = &self.location {
            loc.clone()
        } else {
            // No location -- just use branch/commit ref if provided
            if let Some(ref commit) = self.commit {
                return Ok(format!("commit/{commit}"));
            }
            return match &self.branch {
                Some(branch) => Ok(format!("tree/{branch}")),
                None => Ok(String::new()),
            };
        };

        // Check if location is an issue/PR number
        let trimmed = location.trim_start_matches('#');
        if trimmed.parse::<u64>().is_ok() && git_ref.is_none() {
            return Ok(format!("issues/{trimmed}"));
        }

        // Check if location is a commit SHA (7-64 hex characters)
        if is_commit_sha(&location) && git_ref.is_none() {
            return Ok(format!("commit/{location}"));
        }

        // Parse file path with optional line range (file.go:10 or file.go:10-20)
        let (file_path, range_start, range_end) = parse_file_location(&location)?;

        let ref_name = git_ref.unwrap_or_else(|| default_branch.unwrap_or("HEAD").to_string());

        if range_start > 0 {
            let range_fragment = if range_end > 0 && range_start != range_end {
                format!("L{range_start}-L{range_end}")
            } else {
                format!("L{range_start}")
            };
            Ok(format!(
                "blob/{ref_name}/{file_path}?plain=1#{range_fragment}"
            ))
        } else {
            let path = format!("tree/{ref_name}/{file_path}");
            Ok(path.trim_end_matches('/').to_string())
        }
    }
}

/// Check if a string looks like a commit SHA (7-64 hex characters).
fn is_commit_sha(s: &str) -> bool {
    let len = s.len();
    (7..=64).contains(&len)
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Parse a file location with optional line number or line range.
///
/// Supports formats like:
/// - `main.go` -> ("main.go", 0, 0)
/// - `main.go:10` -> ("main.go", 10, 10)
/// - `main.go:10-20` -> ("main.go", 10, 20)
fn parse_file_location(location: &str) -> Result<(String, u32, u32)> {
    let parts: Vec<&str> = location.splitn(3, ':').collect();

    if parts.len() > 2 {
        anyhow::bail!("invalid file argument: {location:?}");
    }

    let file_path = parts[0].replace('\\', "/");

    if parts.len() < 2 {
        return Ok((file_path, 0, 0));
    }

    let range_str = parts[1];
    if let Some(dash_pos) = range_str.find('-') {
        let start: u32 = range_str[..dash_pos]
            .parse()
            .with_context(|| format!("invalid file argument: {location:?}"))?;
        let end: u32 = range_str[dash_pos + 1..]
            .parse()
            .with_context(|| format!("invalid file argument: {location:?}"))?;
        Ok((file_path, start, end))
    } else {
        let line: u32 = range_str
            .parse()
            .with_context(|| format!("invalid file argument: {location:?}"))?;
        Ok((file_path, line, line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::TestHarness;

    fn browse_args(repo: &str) -> BrowseArgs {
        BrowseArgs {
            location: None,
            repo: Some(repo.to_string()),
            settings: false,
            wiki: false,
            issues: false,
            pulls: false,
            releases: false,
            projects: false,
            actions: false,
            branch: None,
            no_browser: false,
            commit: None,
        }
    }

    #[tokio::test]
    async fn test_should_open_repo_in_browser() {
        let h = TestHarness::new().await;
        let args = browse_args("owner/repo");
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("github.com/owner/repo"));
    }

    #[tokio::test]
    async fn test_should_open_issues_page() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.issues = true;
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].ends_with("/issues"));
    }

    #[tokio::test]
    async fn test_should_open_pulls_page() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.pulls = true;
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].ends_with("/pulls"));
    }

    #[tokio::test]
    async fn test_should_open_settings_page() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.settings = true;
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].ends_with("/settings"));
    }

    #[tokio::test]
    async fn test_should_open_actions_page() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.actions = true;
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].ends_with("/actions"));
    }

    #[tokio::test]
    async fn test_should_print_url_with_no_browser_flag() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.no_browser = true;
        args.run(&h.factory).await.unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("github.com/owner/repo"));
        assert!(h.opened_urls().is_empty());
    }

    #[tokio::test]
    async fn test_should_open_branch_path() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.branch = Some("feature".to_string());
        args.location = Some("src/main.rs".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/tree/feature/src/main.rs"));
    }

    #[tokio::test]
    async fn test_should_open_commit_page() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.commit = Some("abc123def".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/commit/abc123def"));
    }

    #[tokio::test]
    async fn test_should_open_issue_by_number() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("217".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].ends_with("/issues/217"));
    }

    #[tokio::test]
    async fn test_should_open_issue_with_hash_prefix() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("#42".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].ends_with("/issues/42"));
    }

    #[tokio::test]
    async fn test_should_open_commit_sha_from_location() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("77507cd94ccafcf568f8560cfecde965fcfa63".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/commit/77507cd"));
    }

    #[tokio::test]
    async fn test_should_open_file_with_line_number() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("main.go:312".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/blob/HEAD/main.go?plain=1#L312"));
    }

    #[tokio::test]
    async fn test_should_open_file_with_line_range() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("main.go:10-20".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/blob/HEAD/main.go?plain=1#L10-L20"));
    }

    #[tokio::test]
    async fn test_should_open_file_with_line_and_branch() {
        let h = TestHarness::new().await;
        let mut args = browse_args("owner/repo");
        args.location = Some("main.go:10".to_string());
        args.branch = Some("develop".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/blob/develop/main.go?plain=1#L10"));
    }

    #[tokio::test]
    async fn test_should_error_without_repo() {
        let h = TestHarness::new().await;
        let args = BrowseArgs {
            location: None,
            repo: None,
            settings: false,
            wiki: false,
            issues: false,
            pulls: false,
            releases: false,
            projects: false,
            actions: false,
            branch: None,
            no_browser: false,
            commit: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }

    // --- Unit tests for helper functions ---

    #[test]
    fn test_should_detect_commit_sha() {
        assert!(is_commit_sha("abc1234"));
        assert!(is_commit_sha("77507cd94ccafcf568f8560cfecde965fcfa63"));
        assert!(!is_commit_sha("abc12")); // Too short
        assert!(!is_commit_sha("ABC1234")); // Uppercase
        assert!(!is_commit_sha("xyz1234")); // Not hex
        assert!(!is_commit_sha("123")); // Too short
    }

    #[test]
    fn test_should_parse_file_location() {
        let (path, start, end) = parse_file_location("main.go").unwrap();
        assert_eq!(path, "main.go");
        assert_eq!(start, 0);
        assert_eq!(end, 0);
    }

    #[test]
    fn test_should_parse_file_location_with_line() {
        let (path, start, end) = parse_file_location("main.go:312").unwrap();
        assert_eq!(path, "main.go");
        assert_eq!(start, 312);
        assert_eq!(end, 312);
    }

    #[test]
    fn test_should_parse_file_location_with_range() {
        let (path, start, end) = parse_file_location("main.go:10-20").unwrap();
        assert_eq!(path, "main.go");
        assert_eq!(start, 10);
        assert_eq!(end, 20);
    }

    #[test]
    fn test_should_reject_invalid_file_location() {
        assert!(parse_file_location("main.go:10:20:30").is_err());
    }
}
