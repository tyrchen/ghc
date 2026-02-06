//! `ghc pr create` command.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Create a pull request.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Title of the pull request.
    #[arg(short, long)]
    title: Option<String>,

    /// Body of the pull request.
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Base branch (target branch for merge).
    #[arg(short = 'B', long)]
    base: Option<String>,

    /// Head branch (source branch with changes).
    #[arg(short = 'H', long)]
    head: Option<String>,

    /// Create as a draft pull request.
    #[arg(short, long)]
    draft: bool,

    /// Skip prompts and open the text editor to write the title and body.
    #[arg(short, long)]
    editor: bool,

    /// Use commit info for title and body.
    #[arg(short, long = "fill")]
    autofill: bool,

    /// Use commits msg+body for description.
    #[arg(long)]
    fill_verbose: bool,

    /// Use first commit info for title and body.
    #[arg(long)]
    fill_first: bool,

    /// Labels to add.
    #[arg(short, long)]
    label: Vec<String>,

    /// Assignees to add (by login). Use "@me" to self-assign.
    #[arg(short, long)]
    assignee: Vec<String>,

    /// Reviewers to request (by login or team handle).
    #[arg(short, long)]
    reviewer: Vec<String>,

    /// Milestone name or number.
    #[arg(short, long)]
    milestone: Option<String>,

    /// Template file to use as starting body text.
    #[arg(short = 'T', long)]
    template: Option<String>,

    /// Disable maintainer's ability to modify pull request.
    #[arg(long)]
    no_maintainer_edit: bool,

    /// Print details instead of creating the PR.
    #[arg(long)]
    dry_run: bool,

    /// Open in web browser after creating.
    #[arg(short, long)]
    web: bool,
}

impl CreateArgs {
    /// Run the pr create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Determine base branch
        let base = if let Some(b) = &self.base {
            b.clone()
        } else {
            // Fetch default branch from the repo
            let mut vars = HashMap::new();
            vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
            vars.insert("name".to_string(), Value::String(repo.name().to_string()));
            let data: Value = client
                .graphql(
                    r"query DefaultBranch($owner: String!, $name: String!) {
                            repository(owner: $owner, name: $name) {
                                defaultBranchRef { name }
                            }
                        }",
                    &vars,
                )
                .await
                .context("failed to determine default branch")?;
            data.pointer("/repository/defaultBranchRef/name")
                .and_then(Value::as_str)
                .unwrap_or("main")
                .to_string()
        };

        // Determine head branch
        let head = if let Some(h) = &self.head {
            h.clone()
        } else {
            // Try to get current branch from git
            let output = tokio::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .await
                .context("failed to determine current branch")?;
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                anyhow::bail!("could not determine current branch; use --head to specify");
            }
        };

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(
                crate::issue::create::read_body_file(body_file)
                    .context("failed to read body file")?,
            )
        } else {
            None
        };

        // Auto-fill from commit messages if --fill or --fill-verbose
        let (autofill_title, autofill_body) =
            if self.autofill || self.fill_verbose || self.fill_first {
                get_commit_messages(&base, &head, self.fill_verbose, self.fill_first).await?
            } else {
                (None, None)
            };

        // Determine title
        let title = if let Some(ref t) = self.title {
            t.clone()
        } else if let Some(ref t) = autofill_title {
            t.clone()
        } else if self.editor {
            String::new()
        } else {
            let prompter = factory.prompter();
            prompter
                .input("Title", "")
                .context("failed to read title")?
        };

        // Determine body
        let (final_title, final_body) = if self.editor {
            let default_body = body_from_file
                .as_deref()
                .or(autofill_body.as_deref())
                .unwrap_or("");
            let editor_content = format!("{title}\n{default_body}");
            let prompter = factory.prompter();
            let edited = prompter
                .editor("Pull Request", &editor_content, true)
                .context("failed to read from editor")?;
            let mut lines = edited.splitn(2, '\n');
            let t = lines.next().unwrap_or("").trim().to_string();
            let b = lines.next().unwrap_or("").trim().to_string();
            (t, b)
        } else {
            let body = self
                .body
                .clone()
                .or(body_from_file)
                .or(autofill_body)
                .unwrap_or_default();
            (title, body)
        };

        if final_title.is_empty() {
            anyhow::bail!("title is required");
        }

        // Dry run mode - print details and exit
        if self.dry_run {
            ios_eprintln!(ios, "Title: {}", cs.bold(&final_title));
            ios_eprintln!(ios, "Base:  {base}");
            ios_eprintln!(ios, "Head:  {head}");
            ios_eprintln!(ios, "Draft: {}", self.draft);
            if !final_body.is_empty() {
                ios_eprintln!(ios, "Body:\n{final_body}");
            }
            if !self.label.is_empty() {
                ios_eprintln!(ios, "Labels: {}", self.label.join(", "));
            }
            if !self.assignee.is_empty() {
                ios_eprintln!(ios, "Assignees: {}", self.assignee.join(", "));
            }
            if !self.reviewer.is_empty() {
                ios_eprintln!(ios, "Reviewers: {}", self.reviewer.join(", "));
            }
            return Ok(());
        }

        let mut pr_body = serde_json::json!({
            "title": final_title,
            "body": final_body,
            "head": head,
            "base": base,
            "draft": self.draft,
            "maintainer_can_modify": !self.no_maintainer_edit,
        });

        if let Some(ref milestone) = self.milestone {
            // Try to parse as number first, otherwise treat as name
            if let Ok(num) = milestone.parse::<u64>() {
                pr_body["milestone"] = Value::Number(serde_json::Number::from(num));
            }
        }

        let path = format!("repos/{}/{}/pulls", repo.owner(), repo.name());
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&pr_body))
            .await
            .context("failed to create pull request")?;

        let number = result.get("number").and_then(Value::as_i64).unwrap_or(0);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        // Add labels if specified
        if !self.label.is_empty() {
            let labels_body = serde_json::json!({ "labels": self.label });
            let labels_path = format!(
                "repos/{}/{}/issues/{}/labels",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(reqwest::Method::POST, &labels_path, Some(&labels_body))
                .await
                .context("failed to add labels")?;
        }

        // Add assignees if specified
        if !self.assignee.is_empty() {
            let assignees_body = serde_json::json!({ "assignees": self.assignee });
            let assignees_path = format!(
                "repos/{}/{}/issues/{}/assignees",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::POST,
                    &assignees_path,
                    Some(&assignees_body),
                )
                .await
                .context("failed to add assignees")?;
        }

        // Request reviewers if specified
        if !self.reviewer.is_empty() {
            let reviewers_body = serde_json::json!({ "reviewers": self.reviewer });
            let reviewers_path = format!(
                "repos/{}/{}/pulls/{}/requested_reviewers",
                repo.owner(),
                repo.name(),
                number,
            );
            let _: Value = client
                .rest(
                    reqwest::Method::POST,
                    &reviewers_path,
                    Some(&reviewers_body),
                )
                .await
                .context("failed to request reviewers")?;
        }

        ios_eprintln!(
            ios,
            "{} Created pull request #{} in {}",
            cs.success_icon(),
            cs.bold(&number.to_string()),
            repo.full_name(),
        );
        ios_eprintln!(ios, "{html_url}");

        if self.web && !html_url.is_empty() {
            factory.browser().open(html_url)?;
        }

        Ok(())
    }
}

/// Get commit messages between base and head branches for auto-fill.
async fn get_commit_messages(
    base: &str,
    head: &str,
    verbose: bool,
    first_only: bool,
) -> Result<(Option<String>, Option<String>)> {
    let range = format!("{base}..{head}");
    let format_arg = if verbose {
        "--format=%B%n---"
    } else {
        "--format=%s"
    };

    let output = tokio::process::Command::new("git")
        .args(["log", &range, format_arg, "--reverse"])
        .output()
        .await
        .context("failed to get commit messages")?;

    if !output.status.success() {
        return Ok((None, None));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.trim().lines().collect();

    if lines.is_empty() {
        return Ok((None, None));
    }

    if first_only {
        let title = lines.first().map(|l| l.trim().to_string());
        return Ok((title, None));
    }

    if lines.len() == 1 || !verbose {
        let title = lines.first().map(|l| l.trim().to_string());
        let body = if lines.len() > 1 {
            Some(lines[1..].join("\n").trim().to_string())
        } else {
            None
        };
        return Ok((title, body));
    }

    // Verbose mode: first line is title, rest is body
    let title = lines.first().map(|l| l.trim().to_string());
    let body = Some(lines[1..].join("\n").trim().to_string());
    Ok((title, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    fn create_args(repo: &str) -> CreateArgs {
        CreateArgs {
            repo: repo.into(),
            title: Some("New feature".into()),
            body: Some("Description".into()),
            body_file: None,
            base: Some("main".into()),
            head: Some("feature-branch".into()),
            draft: false,
            editor: false,
            autofill: false,
            fill_verbose: false,
            fill_first: false,
            label: vec![],
            assignee: vec![],
            reviewer: vec![],
            milestone: None,
            template: None,
            no_maintainer_edit: false,
            dry_run: false,
            web: false,
        }
    }

    #[tokio::test]
    async fn test_should_create_pull_request() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls",
            201,
            serde_json::json!({
                "number": 10,
                "html_url": "https://github.com/owner/repo/pull/10"
            }),
        )
        .await;

        let args = create_args("owner/repo");
        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Created pull request #10"),
            "should confirm creation: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_create_pr_and_open_in_browser() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/pulls",
            201,
            serde_json::json!({
                "number": 11,
                "html_url": "https://github.com/owner/repo/pull/11"
            }),
        )
        .await;

        let mut args = create_args("owner/repo");
        args.web = true;
        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/pull/11"));
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_create() {
        let h = TestHarness::new().await;
        let args = create_args("bad");
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_should_dry_run_without_creating() {
        let h = TestHarness::new().await;
        let mut args = create_args("owner/repo");
        args.dry_run = true;
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Title:"),
            "should show title in dry run: {err}"
        );
        assert!(err.contains("Base:"), "should show base in dry run: {err}");
        assert!(err.contains("Head:"), "should show head in dry run: {err}");
    }

    #[tokio::test]
    async fn test_should_fail_with_empty_title() {
        let h = TestHarness::new().await;
        let mut args = create_args("owner/repo");
        args.title = Some(String::new());
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("title is required")
        );
    }
}
