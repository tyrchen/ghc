//! `ghc pr view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::text;

/// View a pull request.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Open in web browser.
    #[arg(short, long)]
    web: bool,

    /// Show comments.
    #[arg(short, long)]
    comments: bool,

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
    /// Run the pr view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the PR is not found.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/pull/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.number,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(ghc_api::queries::pr::PR_VIEW_QUERY, &variables)
            .await
            .context("failed to fetch pull request")?;

        let pr = data
            .pointer("/repository/pullRequest")
            .filter(|v| !v.is_null())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not resolve to a PullRequest with the number of {}",
                    self.number,
                )
            })?;

        let ios = &factory.io;
        let cs = ios.color_scheme();

        // JSON output with field filtering, jq, or template
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let mut pr_owned = pr.clone();
            ghc_core::json::normalize_author(&mut pr_owned);
            let output = ghc_core::json::format_json_output(
                &pr_owned,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let title = pr.get("title").and_then(Value::as_str).unwrap_or("");
        let body = pr
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("No description provided");
        let state = pr.get("state").and_then(Value::as_str).unwrap_or("OPEN");
        let is_draft = pr.get("isDraft").and_then(Value::as_bool).unwrap_or(false);
        let author = pr
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let head_ref = pr.get("headRefName").and_then(Value::as_str).unwrap_or("");
        let base_ref = pr.get("baseRefName").and_then(Value::as_str).unwrap_or("");
        let url = pr.get("url").and_then(Value::as_str).unwrap_or("");
        let additions = pr.get("additions").and_then(Value::as_i64).unwrap_or(0);
        let deletions = pr.get("deletions").and_then(Value::as_i64).unwrap_or(0);
        let changed_files = pr.get("changedFiles").and_then(Value::as_i64).unwrap_or(0);
        let review_decision = pr
            .get("reviewDecision")
            .and_then(Value::as_str)
            .unwrap_or("");
        let comment_count = pr
            .pointer("/comments/totalCount")
            .and_then(Value::as_i64)
            .unwrap_or(0);

        // Title and state
        let state_display = if is_draft {
            cs.gray("Draft")
        } else {
            match state {
                "OPEN" => cs.success("Open"),
                "CLOSED" => cs.error("Closed"),
                "MERGED" => cs.magenta("Merged"),
                _ => state.to_string(),
            }
        };

        ios_println!(
            ios,
            "{} #{} {}\n",
            cs.bold(title),
            self.number,
            state_display
        );
        ios_println!(
            ios,
            "{author} wants to merge into {base_ref} from {head_ref}\n"
        );

        // Labels
        if let Some(labels) = pr.pointer("/labels/nodes").and_then(Value::as_array)
            && !labels.is_empty()
        {
            let label_names: Vec<&str> = labels
                .iter()
                .filter_map(|l| l.get("name").and_then(Value::as_str))
                .collect();
            ios_println!(ios, "Labels: {}", label_names.join(", "));
        }

        // Review decision
        if !review_decision.is_empty() {
            let decision_display = match review_decision {
                "APPROVED" => cs.success("Approved"),
                "CHANGES_REQUESTED" => cs.warning("Changes requested"),
                "REVIEW_REQUIRED" => cs.gray("Review required"),
                _ => review_decision.to_string(),
            };
            ios_println!(ios, "Review: {decision_display}");
        }

        // Stats
        ios_println!(
            ios,
            "Changes: {} {} in {} {}",
            cs.success(&format!("+{additions}")),
            cs.error(&format!("-{deletions}")),
            changed_files,
            text::pluralize(changed_files, "file", "files"),
        );
        ios_println!(
            ios,
            "Comments: {}",
            text::pluralize(comment_count, "comment", "comments"),
        );

        // Body
        if !body.is_empty() {
            ios_println!(ios, "\n---");
            let rendered = ghc_core::markdown::render(body, ios.terminal_width());
            ios_println!(ios, "{rendered}");
        }

        ios_println!(ios, "\n{}", text::display_url(url));

        // Show comments if requested
        if self.comments {
            self.print_comments(&client, &repo, ios, &cs).await?;
        }

        Ok(())
    }

    /// Fetch and print PR comments via REST API.
    ///
    /// Fetches issue comments, inline review comments, and top-level review
    /// comments, then displays them in chronological order. This matches
    /// `gh pr view --comments` behavior which shows all comment types.
    #[allow(clippy::too_many_lines)]
    async fn print_comments(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
    ) -> Result<()> {
        // Fetch issue comments (general comments on the PR)
        let issue_path = format!(
            "repos/{}/{}/issues/{}/comments",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let issue_comments: Vec<Value> = client
            .rest(reqwest::Method::GET, &issue_path, None)
            .await
            .context("failed to fetch issue comments")?;

        // Fetch inline review comments (code-level comments)
        let review_comments_path = format!(
            "repos/{}/{}/pulls/{}/comments",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let review_comments: Vec<Value> = client
            .rest(reqwest::Method::GET, &review_comments_path, None)
            .await
            .context("failed to fetch review comments")?;

        // Fetch top-level review comments (review body with status like COMMENTED, APPROVED, etc.)
        let reviews_path = format!(
            "repos/{}/{}/pulls/{}/reviews",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let reviews: Vec<Value> = client
            .rest(reqwest::Method::GET, &reviews_path, None)
            .await
            .context("failed to fetch reviews")?;

        // Convert non-empty review bodies into comment-like objects
        let review_body_comments: Vec<Value> = reviews
            .into_iter()
            .filter(|r| {
                r.get("body")
                    .and_then(Value::as_str)
                    .is_some_and(|b| !b.is_empty())
            })
            .map(|r| {
                let state = r
                    .get("state")
                    .and_then(Value::as_str)
                    .unwrap_or("COMMENTED");
                let mut comment = r.clone();
                // Add a review_state marker so we can display it
                comment.as_object_mut().map(|m| {
                    m.insert("review_state".to_string(), Value::String(state.to_string()))
                });
                comment
            })
            .collect();

        // Merge and sort by submitted_at/created_at
        let mut all_comments: Vec<&Value> = issue_comments
            .iter()
            .chain(review_comments.iter())
            .chain(review_body_comments.iter())
            .collect();
        all_comments.sort_by(|a, b| {
            let a_date = a
                .get("submitted_at")
                .or_else(|| a.get("created_at"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let b_date = b
                .get("submitted_at")
                .or_else(|| b.get("created_at"))
                .and_then(Value::as_str)
                .unwrap_or("");
            a_date.cmp(b_date)
        });

        if all_comments.is_empty() {
            ios_println!(ios, "\n{}", cs.gray("No comments on this pull request."));
            return Ok(());
        }

        ios_println!(ios, "\n{}", cs.bold("Comments:"));
        ios_println!(ios, "{}", "-".repeat(40));

        for comment in &all_comments {
            let author = comment
                .pointer("/user/login")
                .and_then(Value::as_str)
                .unwrap_or("ghost");
            let body = comment.get("body").and_then(Value::as_str).unwrap_or("");
            let timestamp = comment
                .get("submitted_at")
                .or_else(|| comment.get("created_at"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let diff_hunk = comment.get("diff_hunk").and_then(Value::as_str);
            let file_path = comment.get("path").and_then(Value::as_str);
            let review_state = comment.get("review_state").and_then(Value::as_str);

            let action = if let Some(state) = review_state {
                match state {
                    "APPROVED" => "approved",
                    "CHANGES_REQUESTED" => "requested changes",
                    "DISMISSED" => "dismissed review",
                    _ => "commented",
                }
            } else {
                "commented"
            };

            ios_println!(
                ios,
                "\n{} {} {}",
                cs.bold(author),
                action,
                cs.gray(timestamp),
            );

            // Show file context for inline review comments
            if let Some(path) = file_path {
                ios_println!(ios, "{}", cs.gray(&format!("  {path}")));
            }
            if let Some(hunk) = diff_hunk {
                // Show last line of the diff hunk for context
                if let Some(last_line) = hunk.lines().last() {
                    ios_println!(ios, "{}", cs.gray(&format!("  {last_line}")));
                }
            }

            ios_println!(ios, "{body}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql, mock_rest_get};

    fn graphql_pr_view_response(pr: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": pr
                }
            }
        })
    }

    fn pr_view_fixture() -> serde_json::Value {
        serde_json::json!({
            "number": 42,
            "title": "Add logging",
            "body": "This PR adds structured logging.",
            "state": "OPEN",
            "isDraft": false,
            "author": { "login": "testuser" },
            "headRefName": "feature/logging",
            "baseRefName": "main",
            "labels": { "nodes": [{ "name": "enhancement", "color": "0075ca" }] },
            "url": "https://github.com/owner/repo/pull/42",
            "createdAt": "2024-01-15T10:00:00Z",
            "updatedAt": "2024-01-15T12:00:00Z",
            "comments": { "totalCount": 3 },
            "additions": 50,
            "deletions": 10,
            "changedFiles": 5,
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE"
        })
    }

    #[tokio::test]
    async fn test_should_view_pull_request() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(out.contains("Add logging"), "should contain title: {out}");
        assert!(out.contains("#42"), "should contain PR number: {out}");
        assert!(out.contains("main"), "should contain base ref: {out}");
        assert!(out.contains("+50"), "should contain additions: {out}");
    }

    #[tokio::test]
    async fn test_should_open_web_browser_for_pr_view() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: true,
            comments: false,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/pull/42"));
    }

    #[tokio::test]
    async fn test_should_output_json_for_pr_view() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec!["number".into()],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("\"number\":42"),
            "should contain JSON number: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_show_comments_when_flag_set() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/issues/42/comments",
            serde_json::json!([
                {
                    "user": { "login": "reviewer" },
                    "body": "Looks good to me!",
                    "created_at": "2024-01-16T10:00:00Z"
                },
                {
                    "user": { "login": "author" },
                    "body": "Thanks for the review!",
                    "created_at": "2024-01-16T11:00:00Z"
                }
            ]),
        )
        .await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/42/comments",
            serde_json::json!([
                {
                    "user": { "login": "code-reviewer" },
                    "body": "Consider renaming this variable.",
                    "created_at": "2024-01-16T09:00:00Z",
                    "path": "src/main.rs",
                    "diff_hunk": "@@ -10,6 +10,8 @@\n+let foo = bar();"
                }
            ]),
        )
        .await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/pulls/42/reviews",
            serde_json::json!([
                {
                    "user": { "login": "lead-reviewer" },
                    "body": "Overall looks great, ship it!",
                    "state": "APPROVED",
                    "submitted_at": "2024-01-16T12:00:00Z"
                }
            ]),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: true,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("reviewer"),
            "should contain comment author: {out}"
        );
        assert!(
            out.contains("Looks good to me!"),
            "should contain comment body: {out}"
        );
        assert!(
            out.contains("Thanks for the review!"),
            "should contain second comment: {out}"
        );
        assert!(
            out.contains("code-reviewer"),
            "should contain review comment author: {out}"
        );
        assert!(
            out.contains("Consider renaming"),
            "should contain review comment body: {out}"
        );
        assert!(
            out.contains("lead-reviewer"),
            "should contain review body author: {out}"
        );
        assert!(
            out.contains("Overall looks great"),
            "should contain review body text: {out}"
        );
        assert!(
            out.contains("approved"),
            "should contain review state: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_apply_jq_filter() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestView",
            graphql_pr_view_response(&pr_view_fixture()),
        )
        .await;

        let args = ViewArgs {
            number: 42,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
            jq: Some(".title".into()),
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let out = h.stdout();
        assert!(
            out.contains("Add logging"),
            "should contain jq-filtered title: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_pr_not_found() {
        let h = TestHarness::new().await;
        // When pullRequest is missing entirely, pointer returns None
        let response = serde_json::json!({
            "data": {
                "repository": {}
            }
        });
        mock_graphql(&h.server, "PullRequestView", response).await;

        let args = ViewArgs {
            number: 999,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Could not resolve"),
            "should report not found error",
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_pr_null() {
        let h = TestHarness::new().await;
        // When pullRequest is null (nonexistent PR number), filter catches it
        let response = serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": null
                }
            }
        });
        mock_graphql(&h.server, "PullRequestView", response).await;

        let args = ViewArgs {
            number: 99999,
            repo: "owner/repo".into(),
            web: false,
            comments: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Could not resolve"),
            "should report not found error for null PR",
        );
    }
}
