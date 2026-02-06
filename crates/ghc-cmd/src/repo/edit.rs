//! `ghc repo edit` command.

use std::collections::HashSet;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// Edit repository settings.
///
/// To toggle a setting off, use the `--<flag>=false` syntax.
///
/// Changing repository visibility can have unexpected consequences including but
/// not limited to: losing stars and watchers, detaching public forks from the
/// network, disabling push rulesets, and allowing access to GitHub Actions
/// history and logs.
///
/// When the `--visibility` flag is used, `--accept-visibility-change-consequences`
/// flag is required.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct EditArgs {
    /// Repository to edit (OWNER/REPO).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// Description of the repository.
    #[arg(short, long)]
    description: Option<String>,

    /// Repository home page URL.
    #[arg(short = 'h', long)]
    homepage: Option<String>,

    /// Set the default branch name for the repository.
    #[arg(long)]
    default_branch: Option<String>,

    /// Change the visibility of the repository to {public,private,internal}.
    #[arg(long)]
    visibility: Option<String>,

    /// Make the repository available as a template repository.
    #[arg(long)]
    template: Option<bool>,

    /// Enable issues in the repository.
    #[arg(long)]
    enable_issues: Option<bool>,

    /// Enable projects in the repository.
    #[arg(long)]
    enable_projects: Option<bool>,

    /// Enable wiki in the repository.
    #[arg(long)]
    enable_wiki: Option<bool>,

    /// Enable discussions in the repository.
    #[arg(long)]
    enable_discussions: Option<bool>,

    /// Enable merging pull requests via merge commit.
    #[arg(long)]
    enable_merge_commit: Option<bool>,

    /// Enable merging pull requests via squashed commit.
    #[arg(long)]
    enable_squash_merge: Option<bool>,

    /// Enable merging pull requests via rebase.
    #[arg(long)]
    enable_rebase_merge: Option<bool>,

    /// Enable auto-merge functionality.
    #[arg(long)]
    enable_auto_merge: Option<bool>,

    /// Enable advanced security in the repository.
    #[arg(long)]
    enable_advanced_security: Option<bool>,

    /// Enable secret scanning in the repository.
    #[arg(long)]
    enable_secret_scanning: Option<bool>,

    /// Enable secret scanning push protection in the repository.
    #[arg(long)]
    enable_secret_scanning_push_protection: Option<bool>,

    /// Delete head branch when pull requests are merged.
    #[arg(long)]
    delete_branch_on_merge: Option<bool>,

    /// Allow forking of an organization repository.
    #[arg(long)]
    allow_forking: Option<bool>,

    /// Allow a pull request head branch that is behind its base branch to be updated.
    #[arg(long)]
    allow_update_branch: Option<bool>,

    /// Add repository topic.
    #[arg(long = "add-topic", value_delimiter = ',')]
    add_topics: Vec<String>,

    /// Remove repository topic.
    #[arg(long = "remove-topic", value_delimiter = ',')]
    remove_topics: Vec<String>,

    /// Accept the consequences of changing the repository visibility.
    #[arg(long)]
    accept_visibility_change_consequences: bool,
}

impl EditArgs {
    /// Run the repo edit command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let repo = match &self.repo {
            Some(r) => Repo::from_full_name(r).context("invalid repository format")?,
            None => {
                anyhow::bail!("repository argument required (e.g. OWNER/REPO)")
            }
        };

        if self.visibility.is_some() && !self.accept_visibility_change_consequences {
            anyhow::bail!(
                "use of --visibility flag requires --accept-visibility-change-consequences flag"
            );
        }

        let client = factory.api_client(repo.host())?;
        let api_path = format!("repos/{}/{}", repo.owner(), repo.name());

        self.patch_repo_settings(&client, &api_path).await?;
        self.update_topics(&client, &repo).await?;

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Edited repository {}",
                cs.success_icon(),
                repo.full_name()
            );
        }

        Ok(())
    }

    async fn patch_repo_settings(
        &self,
        client: &ghc_api::client::Client,
        api_path: &str,
    ) -> Result<()> {
        let mut body = serde_json::Map::new();

        self.insert_string_fields(&mut body);
        self.insert_bool_fields(&mut body);
        self.insert_security_fields(&mut body);

        if !body.is_empty() {
            let body_value = Value::Object(body);
            let _result: Value = client
                .rest(reqwest::Method::PATCH, api_path, Some(&body_value))
                .await
                .context("failed to edit repository")?;
        }
        Ok(())
    }

    fn insert_string_fields(&self, body: &mut serde_json::Map<String, Value>) {
        if let Some(ref desc) = self.description {
            body.insert("description".into(), Value::String(desc.clone()));
        }
        if let Some(ref hp) = self.homepage {
            body.insert("homepage".into(), Value::String(hp.clone()));
        }
        if let Some(ref branch) = self.default_branch {
            body.insert("default_branch".into(), Value::String(branch.clone()));
        }
        if let Some(ref vis) = self.visibility {
            body.insert("visibility".into(), Value::String(vis.clone()));
        }
    }

    fn insert_bool_fields(&self, body: &mut serde_json::Map<String, Value>) {
        let fields: &[(&str, Option<bool>)] = &[
            ("is_template", self.template),
            ("has_issues", self.enable_issues),
            ("has_projects", self.enable_projects),
            ("has_wiki", self.enable_wiki),
            ("has_discussions", self.enable_discussions),
            ("allow_merge_commit", self.enable_merge_commit),
            ("allow_squash_merge", self.enable_squash_merge),
            ("allow_rebase_merge", self.enable_rebase_merge),
            ("allow_auto_merge", self.enable_auto_merge),
            ("delete_branch_on_merge", self.delete_branch_on_merge),
            ("allow_forking", self.allow_forking),
            ("allow_update_branch", self.allow_update_branch),
        ];
        for &(key, value) in fields {
            if let Some(v) = value {
                body.insert(key.into(), Value::Bool(v));
            }
        }
    }

    fn insert_security_fields(&self, body: &mut serde_json::Map<String, Value>) {
        if self.enable_advanced_security.is_none()
            && self.enable_secret_scanning.is_none()
            && self.enable_secret_scanning_push_protection.is_none()
        {
            return;
        }

        let mut security = serde_json::Map::new();
        if let Some(v) = self.enable_advanced_security {
            security.insert(
                "advanced_security".into(),
                serde_json::json!({"status": bool_to_status(v)}),
            );
        }
        if let Some(v) = self.enable_secret_scanning {
            security.insert(
                "secret_scanning".into(),
                serde_json::json!({"status": bool_to_status(v)}),
            );
        }
        if let Some(v) = self.enable_secret_scanning_push_protection {
            security.insert(
                "secret_scanning_push_protection".into(),
                serde_json::json!({"status": bool_to_status(v)}),
            );
        }
        body.insert("security_and_analysis".into(), Value::Object(security));
    }

    async fn update_topics(&self, client: &ghc_api::client::Client, repo: &Repo) -> Result<()> {
        if self.add_topics.is_empty() && self.remove_topics.is_empty() {
            return Ok(());
        }

        let topics_path = format!("repos/{}/{}/topics", repo.owner(), repo.name());
        let current_topics: TopicsResponse = client
            .rest(reqwest::Method::GET, &topics_path, None)
            .await
            .context("failed to get repository topics")?;

        let mut topic_set: HashSet<String> = current_topics.names.into_iter().collect();

        for topic in &self.add_topics {
            topic_set.insert(topic.trim().to_string());
        }
        for topic in &self.remove_topics {
            topic_set.remove(topic.trim());
        }

        let new_topics: Vec<String> = topic_set.into_iter().collect();
        let topics_body = serde_json::json!({ "names": new_topics });
        let _: Value = client
            .rest(reqwest::Method::PUT, &topics_path, Some(&topics_body))
            .await
            .context("failed to update repository topics")?;

        Ok(())
    }
}

#[derive(Debug, serde::Deserialize)]
struct TopicsResponse {
    names: Vec<String>,
}

fn bool_to_status(v: bool) -> &'static str {
    if v { "enabled" } else { "disabled" }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_edit_repository_description() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/repo",
            200,
            serde_json::json!({
                "full_name": "owner/repo",
                "description": "Updated description",
            }),
        )
        .await;

        let args = EditArgs {
            repo: Some("owner/repo".into()),
            description: Some("Updated description".into()),
            homepage: None,
            default_branch: None,
            visibility: None,
            template: None,
            enable_issues: None,
            enable_projects: None,
            enable_wiki: None,
            enable_discussions: None,
            enable_merge_commit: None,
            enable_squash_merge: None,
            enable_rebase_merge: None,
            enable_auto_merge: None,
            enable_advanced_security: None,
            enable_secret_scanning: None,
            enable_secret_scanning_push_protection: None,
            delete_branch_on_merge: None,
            allow_forking: None,
            allow_update_branch: None,
            add_topics: vec![],
            remove_topics: vec![],
            accept_visibility_change_consequences: false,
        };
        // Succeeds without error (TTY output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }

    #[tokio::test]
    async fn test_should_fail_visibility_without_acceptance() {
        let h = TestHarness::new().await;

        let args = EditArgs {
            repo: Some("owner/repo".into()),
            description: None,
            homepage: None,
            default_branch: None,
            visibility: Some("private".into()),
            template: None,
            enable_issues: None,
            enable_projects: None,
            enable_wiki: None,
            enable_discussions: None,
            enable_merge_commit: None,
            enable_squash_merge: None,
            enable_rebase_merge: None,
            enable_auto_merge: None,
            enable_advanced_security: None,
            enable_secret_scanning: None,
            enable_secret_scanning_push_protection: None,
            delete_branch_on_merge: None,
            allow_forking: None,
            allow_update_branch: None,
            add_topics: vec![],
            remove_topics: vec![],
            accept_visibility_change_consequences: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("accept-visibility-change-consequences")
        );
    }

    #[test]
    fn test_should_convert_bool_to_status() {
        assert_eq!(bool_to_status(true), "enabled");
        assert_eq!(bool_to_status(false), "disabled");
    }
}
