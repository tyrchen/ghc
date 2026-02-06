//! `ghc pr merge` command.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Merge method for a pull request.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum MergeMethod {
    /// Standard merge commit.
    Merge,
    /// Squash and merge into a single commit.
    Squash,
    /// Rebase and merge without a merge commit.
    Rebase,
}

/// Merge a pull request.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct MergeArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Merge method to use (alternative to --merge/--squash/--rebase).
    #[arg(long, value_enum, conflicts_with_all = ["merge_flag", "squash", "rebase"])]
    method: Option<MergeMethod>,

    /// Merge via a merge commit.
    #[arg(short = 'm', long = "merge", conflicts_with_all = ["squash", "rebase"])]
    merge_flag: bool,

    /// Squash commits into a single commit before merging.
    #[arg(short = 's', long, conflicts_with_all = ["merge_flag", "rebase"])]
    squash: bool,

    /// Rebase commits before merging.
    #[arg(short = 'r', long, conflicts_with_all = ["merge_flag", "squash"])]
    rebase: bool,

    /// Commit title for the merge commit.
    #[arg(short = 't', long)]
    subject: Option<String>,

    /// Commit message body for the merge commit.
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Delete the branch after merging.
    #[arg(short, long)]
    delete_branch: bool,

    /// Enable auto-merge when requirements are met.
    #[arg(long)]
    auto: bool,

    /// Disable auto-merge for this pull request.
    #[arg(long)]
    disable_auto: bool,

    /// Use administrator privileges to merge a pull request that does not meet requirements.
    #[arg(long)]
    admin: bool,

    /// Commit SHA that the pull request head must match to allow merge.
    #[arg(long)]
    match_head_commit: Option<String>,

    /// Email for merge commit author.
    #[arg(short = 'A', long)]
    author_email: Option<String>,
}

impl MergeArgs {
    /// Resolve the merge method from boolean flags or --method.
    /// Returns `None` if no explicit method was specified.
    fn explicit_merge_method(&self) -> Option<MergeMethod> {
        if self.merge_flag {
            Some(MergeMethod::Merge)
        } else if self.squash {
            Some(MergeMethod::Squash)
        } else if self.rebase {
            Some(MergeMethod::Rebase)
        } else {
            self.method.clone()
        }
    }

    /// Run the pr merge command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Handle auto-merge enable/disable via GraphQL
        if self.auto || self.disable_auto {
            return self.handle_auto_merge(&client, &repo, ios).await;
        }

        // Handle --admin: use GraphQL mergePullRequest mutation
        if self.admin {
            return self.handle_admin_merge(&client, &repo, ios).await;
        }

        // Resolve merge method: boolean flags, --method, prompt, or default
        let resolved_method = if let Some(m) = self.explicit_merge_method() {
            m
        } else if ios.can_prompt() {
            let options = vec![
                "Create a merge commit".to_string(),
                "Rebase and merge".to_string(),
                "Squash and merge".to_string(),
            ];
            let prompter = factory.prompter();
            let selection = prompter
                .select("Merge method", Some(0), &options)
                .context("failed to select merge method")?;
            match selection {
                1 => MergeMethod::Rebase,
                2 => MergeMethod::Squash,
                _ => MergeMethod::Merge,
            }
        } else {
            MergeMethod::Merge
        };

        let merge_method = match resolved_method {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
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

        let mut body = serde_json::json!({
            "merge_method": merge_method,
        });

        if let Some(ref subject) = self.subject {
            body["commit_title"] = Value::String(subject.clone());
        }
        if let Some(ref msg) = self.body {
            body["commit_message"] = Value::String(msg.clone());
        } else if let Some(ref msg) = body_from_file {
            body["commit_message"] = Value::String(msg.clone());
        }
        if let Some(ref sha) = self.match_head_commit {
            body["sha"] = Value::String(sha.clone());
        }

        let path = format!(
            "repos/{}/{}/pulls/{}/merge",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let result: Value = client
            .rest(reqwest::Method::PUT, &path, Some(&body))
            .await
            .context("failed to merge pull request")?;

        let merged = result
            .get("merged")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if merged {
            ios_eprintln!(
                ios,
                "{} Merged pull request #{} via {merge_method}",
                cs.success_icon(),
                self.number,
            );
        } else {
            let message = result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown reason");
            anyhow::bail!(
                "pull request #{} could not be merged: {message}",
                self.number,
            );
        }

        // Delete branch if requested
        if self.delete_branch {
            self.delete_head_branch(&client, &repo, ios).await?;
        }

        Ok(())
    }

    /// Handle --admin merge via GraphQL mutation (bypasses requirements).
    async fn handle_admin_merge(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();

        // Get the PR node ID
        let mut vars = std::collections::HashMap::new();
        vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let pr_data: Value = client
            .graphql(
                r"query PrNodeId($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) {
                        pullRequest(number: $number) { id }
                    }
                }",
                &vars,
            )
            .await
            .context("failed to fetch pull request ID")?;

        let pr_id = pr_data
            .pointer("/repository/pullRequest/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow::anyhow!("pull request #{} not found", self.number))?;

        let merge_method = match self.explicit_merge_method() {
            Some(MergeMethod::Squash) => "SQUASH",
            Some(MergeMethod::Rebase) => "REBASE",
            Some(MergeMethod::Merge) | None => "MERGE",
        };

        let mut mutation_vars = std::collections::HashMap::new();
        mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));
        mutation_vars.insert(
            "mergeMethod".to_string(),
            Value::String(merge_method.to_string()),
        );

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(
                crate::issue::create::read_body_file(body_file)
                    .context("failed to read body file")?,
            )
        } else {
            None
        };

        let commit_body = self
            .body
            .as_ref()
            .or(body_from_file.as_ref())
            .cloned()
            .unwrap_or_default();

        let mut mutation_input = "pullRequestId: $prId, mergeMethod: $mergeMethod".to_string();

        if let Some(ref subject) = self.subject {
            mutation_vars.insert("commitHeadline".to_string(), Value::String(subject.clone()));
            mutation_input.push_str(", commitHeadline: $commitHeadline");
        }

        if !commit_body.is_empty() {
            mutation_vars.insert("commitBody".to_string(), Value::String(commit_body));
            mutation_input.push_str(", commitBody: $commitBody");
        }

        if let Some(ref email) = self.author_email {
            mutation_vars.insert("authorEmail".to_string(), Value::String(email.clone()));
            mutation_input.push_str(", authorEmail: $authorEmail");
        }

        // Build dynamic variable declaration
        let mut var_decl = "$prId: ID!, $mergeMethod: PullRequestMergeMethod!".to_string();
        if self.subject.is_some() {
            var_decl.push_str(", $commitHeadline: String");
        }
        if self.body.is_some() || body_from_file.is_some() {
            var_decl.push_str(", $commitBody: String");
        }
        if self.author_email.is_some() {
            var_decl.push_str(", $authorEmail: String");
        }

        let mutation = format!(
            r"mutation MergePR({var_decl}) {{
                mergePullRequest(input: {{ {mutation_input} }}) {{
                    pullRequest {{ number }}
                }}
            }}",
        );

        let _: Value = client
            .graphql(&mutation, &mutation_vars)
            .await
            .context("failed to merge pull request with admin privileges")?;

        ios_eprintln!(
            ios,
            "{} Merged pull request #{} via {} (admin)",
            cs.success_icon(),
            self.number,
            merge_method.to_lowercase(),
        );

        // Delete branch if requested
        if self.delete_branch {
            self.delete_head_branch(client, repo, ios).await?;
        }

        Ok(())
    }

    /// Delete the head branch of a pull request.
    async fn delete_head_branch(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();
        let pr_path = format!(
            "repos/{}/{}/pulls/{}",
            repo.owner(),
            repo.name(),
            self.number,
        );
        let pr_data: Value = client
            .rest(reqwest::Method::GET, &pr_path, None::<&Value>)
            .await
            .context("failed to fetch pull request details")?;

        let head_ref = pr_data
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .unwrap_or("");

        if !head_ref.is_empty() {
            let ref_path = format!(
                "repos/{}/{}/git/refs/heads/{}",
                repo.owner(),
                repo.name(),
                head_ref,
            );
            let _ = client
                .rest_text(reqwest::Method::DELETE, &ref_path, None)
                .await;
            ios_eprintln!(ios, "{} Deleted branch {head_ref}", cs.success_icon());
        }
        Ok(())
    }

    /// Handle auto-merge enable/disable via GraphQL mutations.
    async fn handle_auto_merge(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> Result<()> {
        let cs = ios.color_scheme();
        // First get the PR node ID
        let mut vars = std::collections::HashMap::new();
        vars.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        vars.insert("name".to_string(), Value::String(repo.name().to_string()));
        vars.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let pr_data: Value = client
            .graphql(
                r"query PrNodeId($owner: String!, $name: String!, $number: Int!) {
                    repository(owner: $owner, name: $name) {
                        pullRequest(number: $number) { id }
                    }
                }",
                &vars,
            )
            .await
            .context("failed to fetch pull request ID")?;

        let pr_id = pr_data
            .pointer("/repository/pullRequest/id")
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow::anyhow!("pull request #{} not found", self.number))?;

        if self.disable_auto {
            let mut mutation_vars = std::collections::HashMap::new();
            mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));

            let _: Value = client
                .graphql(
                    r"mutation DisableAutoMerge($prId: ID!) {
                        disablePullRequestAutoMerge(input: { pullRequestId: $prId }) {
                            pullRequest { number }
                        }
                    }",
                    &mutation_vars,
                )
                .await
                .context("failed to disable auto-merge")?;

            ios_eprintln!(
                ios,
                "{} Disabled auto-merge for pull request #{}",
                cs.success_icon(),
                self.number,
            );
        } else {
            let merge_method = match self.method {
                Some(MergeMethod::Merge) | None => "MERGE",
                Some(MergeMethod::Squash) => "SQUASH",
                Some(MergeMethod::Rebase) => "REBASE",
            };

            let mut mutation_vars = std::collections::HashMap::new();
            mutation_vars.insert("prId".to_string(), Value::String(pr_id.to_string()));
            mutation_vars.insert(
                "mergeMethod".to_string(),
                Value::String(merge_method.to_string()),
            );

            let _: Value = client
                .graphql(
                    r"mutation EnableAutoMerge($prId: ID!, $mergeMethod: PullRequestMergeMethod!) {
                        enablePullRequestAutoMerge(input: { pullRequestId: $prId, mergeMethod: $mergeMethod }) {
                            pullRequest { number }
                        }
                    }",
                    &mutation_vars,
                )
                .await
                .context("failed to enable auto-merge")?;

            ios_eprintln!(
                ios,
                "{} Enabled auto-merge for pull request #{} ({merge_method})",
                cs.success_icon(),
                self.number,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TestHarness;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    fn default_merge_args(number: i64, repo: &str) -> MergeArgs {
        MergeArgs {
            number,
            repo: repo.into(),
            method: Some(MergeMethod::Merge),
            merge_flag: false,
            squash: false,
            rebase: false,
            subject: None,
            body: None,
            body_file: None,
            delete_branch: false,
            auto: false,
            disable_auto: false,
            admin: false,
            match_head_commit: None,
            author_email: None,
        }
    }

    #[tokio::test]
    async fn test_should_merge_pull_request() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/8/merge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "merged": true, "sha": "abc123" })),
            )
            .mount(&h.server)
            .await;

        let args = default_merge_args(8, "owner/repo");
        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("Merged pull request #8"),
            "should confirm merge: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_merge_with_squash_method() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/9/merge"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "merged": true, "sha": "def456" })),
            )
            .mount(&h.server)
            .await;

        let mut args = default_merge_args(9, "owner/repo");
        args.method = Some(MergeMethod::Squash);
        args.subject = Some("Squash commit".into());
        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("via squash"),
            "should mention squash method: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_fail_when_merge_not_possible() {
        let h = TestHarness::new().await;

        Mock::given(method("PUT"))
            .and(path("/repos/owner/repo/pulls/10/merge"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "merged": false, "message": "checks pending" }),
                ),
            )
            .mount(&h.server)
            .await;

        let args = default_merge_args(10, "owner/repo");
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("could not be merged")
        );
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_merge() {
        let h = TestHarness::new().await;
        let args = default_merge_args(1, "bad");
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
