//! `ghc issue develop` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// GraphQL query to fetch issue details and default branch for development.
const ISSUE_FOR_DEVELOP_QUERY: &str = r"
    query IssueForDevelop($owner: String!, $name: String!, $number: Int!) {
      repository(owner: $owner, name: $name) {
        issue(number: $number) {
          id
          title
        }
        defaultBranchRef {
          name
          target {
            oid
          }
        }
      }
    }
";

/// Create a branch linked to an issue for development.
///
/// This creates a new branch from the repository's default branch and
/// links it to the specified issue. The branch name is derived from the
/// issue number and title unless overridden with `--name`.
#[derive(Debug, Args)]
pub struct DevelopArgs {
    /// Issue number to develop on.
    #[arg(value_name = "NUMBER")]
    number: i32,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Custom branch name. Defaults to a name derived from the issue.
    #[arg(short, long)]
    name: Option<String>,

    /// Base branch to create the new branch from. Defaults to the
    /// repository's default branch.
    #[arg(short, long)]
    base: Option<String>,

    /// Checkout the branch locally after creation.
    #[arg(short, long)]
    checkout: bool,

    /// Repository where the branch will be created (OWNER/REPO).
    /// Defaults to the issue repository. Useful for creating branches in forks.
    #[arg(long, conflicts_with = "list")]
    branch_repo: Option<String>,

    /// List branches linked to the issue instead of creating one.
    #[arg(short, long, conflicts_with_all = ["branch_repo", "base", "name", "checkout"])]
    list: bool,
}

impl DevelopArgs {
    /// Run the issue develop command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, the issue is not
    /// found, the branch cannot be created, or the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if self.list {
            return self.run_list(&client, &repo, ios).await;
        }

        // Fetch issue details to build branch name
        let mut issue_vars = HashMap::new();
        issue_vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        issue_vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        issue_vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(ISSUE_FOR_DEVELOP_QUERY, &issue_vars)
            .await
            .context("failed to fetch issue details")?;

        let issue_title = data
            .pointer("/repository/issue/title")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!("issue #{} not found in {}", self.number, repo.full_name())
            })?;

        let default_branch = data
            .pointer("/repository/defaultBranchRef/name")
            .and_then(Value::as_str)
            .unwrap_or("main");

        let base_branch = self.base.as_deref().unwrap_or(default_branch);

        // Build the branch name
        let branch_name = if let Some(name) = &self.name {
            name.clone()
        } else {
            let slug = slugify_title(issue_title);
            format!("{}-{slug}", self.number)
        };

        // Determine which repo to create the branch in
        let target_repo = if let Some(ref branch_repo_name) = self.branch_repo {
            ghc_core::repo::Repo::from_full_name(branch_repo_name)
                .context("invalid --branch-repo format")?
        } else {
            repo.clone()
        };

        // Get the SHA of the base branch (from the issue repo)
        let ref_path = format!(
            "repos/{}/{}/git/ref/heads/{}",
            repo.owner(),
            repo.name(),
            base_branch,
        );

        let ref_data: Value = client
            .rest(reqwest::Method::GET, &ref_path, None::<&Value>)
            .await
            .context("failed to fetch base branch reference")?;

        let sha = ref_data
            .pointer("/object/sha")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("could not determine SHA of branch {base_branch}"))?;

        // Create the branch via REST API in the target repo
        let create_ref_path = format!(
            "repos/{}/{}/git/refs",
            target_repo.owner(),
            target_repo.name(),
        );
        let create_body = serde_json::json!({
            "ref": format!("refs/heads/{branch_name}"),
            "sha": sha,
        });

        let _: Value = client
            .rest(reqwest::Method::POST, &create_ref_path, Some(&create_body))
            .await
            .context("failed to create branch")?;

        ios_eprintln!(
            ios,
            "{} Created branch {} for issue #{} in {}",
            cs.success_icon(),
            cs.bold(&branch_name),
            self.number,
            cs.bold(&target_repo.full_name()),
        );

        // Checkout locally if requested
        if self.checkout {
            let git = factory.git_client()?;
            git.fetch("origin", &branch_name)
                .await
                .context("failed to fetch the new branch")?;
            git.checkout(&branch_name)
                .await
                .context("failed to checkout branch")?;

            ios_eprintln!(
                ios,
                "{} Switched to branch {}",
                cs.success_icon(),
                cs.bold(&branch_name),
            );
        }

        Ok(())
    }

    /// List branches linked to an issue via the Timeline events API.
    async fn run_list(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/issues/{}/timeline?per_page=100",
            repo.owner(),
            repo.name(),
            self.number,
        );

        let events: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to fetch issue timeline")?;

        let mut branches: Vec<String> = Vec::new();
        for event in &events {
            let event_type = event.get("event").and_then(Value::as_str).unwrap_or("");
            if event_type == "cross-referenced"
                && let Some(ref_name) = event
                    .pointer("/source/issue/pull_request/html_url")
                    .and_then(Value::as_str)
                && let Some(head_ref) = event
                    .pointer("/source/issue/pull_request/head/ref")
                    .and_then(Value::as_str)
            {
                branches.push(format!("{head_ref} (PR: {ref_name})"));
            }
            if event_type == "referenced"
                && let Some(ref_name) = event.get("commit_id").and_then(Value::as_str)
            {
                let short_sha = &ref_name[..7.min(ref_name.len())];
                branches.push(format!("commit {short_sha}"));
            }
        }

        if branches.is_empty() {
            ios_eprintln!(
                ios,
                "No linked branches found for issue #{} in {}",
                self.number,
                cs.bold(&repo.full_name()),
            );
        } else {
            ios_eprintln!(
                ios,
                "Branches linked to issue #{} in {}:",
                self.number,
                cs.bold(&repo.full_name()),
            );
            for branch in &branches {
                ghc_core::ios_println!(ios, "  {branch}");
            }
        }

        Ok(())
    }
}

/// Convert an issue title into a URL-safe branch slug.
///
/// Lowercases the input, replaces non-alphanumeric characters with hyphens,
/// collapses consecutive hyphens, and trims leading/trailing hyphens.
fn slugify_title(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = false;
    for ch in slug.chars() {
        if ch == '-' {
            if !prev_hyphen {
                result.push(ch);
            }
            prev_hyphen = true;
        } else {
            result.push(ch);
            prev_hyphen = false;
        }
    }

    let trimmed = result.trim_matches('-');

    // Limit length to keep branch names reasonable
    if trimmed.len() > 50 {
        let truncated = &trimmed[..50];
        truncated.trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_slugify_simple_title() {
        assert_eq!(slugify_title("Fix the bug"), "fix-the-bug");
    }

    #[test]
    fn test_should_slugify_title_with_special_chars() {
        assert_eq!(
            slugify_title("Add support for [feature] (v2)"),
            "add-support-for-feature-v2",
        );
    }

    #[test]
    fn test_should_collapse_consecutive_hyphens() {
        assert_eq!(slugify_title("hello   world"), "hello-world");
    }

    #[test]
    fn test_should_trim_leading_trailing_hyphens() {
        assert_eq!(
            slugify_title("--leading and trailing--"),
            "leading-and-trailing"
        );
    }

    #[test]
    fn test_should_truncate_long_titles() {
        let long_title = "a".repeat(100);
        let slug = slugify_title(&long_title);
        assert!(slug.len() <= 50);
    }

    #[test]
    fn test_should_handle_empty_title() {
        assert_eq!(slugify_title(""), "");
    }
}
