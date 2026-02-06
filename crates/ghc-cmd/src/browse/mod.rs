//! Browse command (`ghc browse`).
//!
//! Open a repository in the web browser.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::{ios_eprintln, ios_println};

/// Open a GitHub repository in the web browser.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct BrowseArgs {
    /// The file or path to browse (optional).
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
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo_str = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository required (use -R OWNER/REPO)"))?;

        let repo =
            ghc_core::repo::Repo::from_full_name(repo_str).context("invalid repository format")?;

        let base_url = format!("https://{}/{}/{}", repo.host(), repo.owner(), repo.name());

        let url = if self.settings {
            format!("{base_url}/settings")
        } else if self.wiki {
            format!("{base_url}/wiki")
        } else if self.issues {
            format!("{base_url}/issues")
        } else if self.pulls {
            format!("{base_url}/pulls")
        } else if self.releases {
            format!("{base_url}/releases")
        } else if self.projects {
            format!("{base_url}/projects")
        } else if let Some(ref commit) = self.commit {
            format!("{base_url}/commit/{commit}")
        } else if let Some(ref location) = self.location {
            let branch = self.branch.as_deref().unwrap_or("HEAD");
            format!("{base_url}/tree/{branch}/{location}")
        } else if let Some(ref branch) = self.branch {
            format!("{base_url}/tree/{branch}")
        } else {
            base_url
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
        args.commit = Some("abc123".to_string());
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert!(urls[0].contains("/commit/abc123"));
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
            branch: None,
            no_browser: false,
            commit: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
