//! `ghc repo view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::text;

/// View a repository.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Repository to view (OWNER/REPO).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// Open in web browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the repo view command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = match &self.repo {
            Some(r) => {
                ghc_core::repo::Repo::from_full_name(r).context("invalid repository format")?
            }
            None => {
                anyhow::bail!("repository argument required (e.g. OWNER/REPO)")
            }
        };

        if self.web {
            let url = format!("https://{}/{}/{}", repo.host(), repo.owner(), repo.name());
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));

        let data: Value = client
            .graphql(ghc_api::queries::repo::REPO_QUERY, &variables)
            .await
            .context("failed to fetch repository")?;

        let repo_data = data
            .get("repository")
            .ok_or_else(|| anyhow::anyhow!("repository not found: {}", repo.full_name()))?;

        let ios = &factory.io;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(repo_data)?);
            return Ok(());
        }

        let cs = ios.color_scheme();

        let name = repo_data.get("name").and_then(Value::as_str).unwrap_or("");
        let owner_login = repo_data
            .pointer("/owner/login")
            .and_then(Value::as_str)
            .unwrap_or("");
        let description = repo_data
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("No description");
        let url = repo_data.get("url").and_then(Value::as_str).unwrap_or("");
        let is_private = repo_data
            .get("isPrivate")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_fork = repo_data
            .get("isFork")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let is_archived = repo_data
            .get("isArchived")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let stars = repo_data
            .get("stargazerCount")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let forks = repo_data
            .get("forkCount")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let default_branch = repo_data
            .pointer("/defaultBranchRef/name")
            .and_then(Value::as_str)
            .unwrap_or("main");
        let language = repo_data
            .pointer("/primaryLanguage/name")
            .and_then(Value::as_str)
            .unwrap_or("");

        ios_println!(
            ios,
            "{}\n{}\n",
            cs.bold(&format!("{owner_login}/{name}")),
            description
        );

        let visibility = if is_private {
            cs.warning("private")
        } else {
            cs.success("public")
        };
        let mut badges = vec![visibility];
        if is_fork {
            badges.push("fork".to_string());
        }
        if is_archived {
            badges.push(cs.warning("archived"));
        }
        ios_println!(ios, "{}", badges.join(" | "));

        if !language.is_empty() {
            ios_println!(ios, "Language: {language}");
        }
        ios_println!(ios, "Stars: {stars}  Forks: {forks}");
        ios_println!(ios, "Default branch: {default_branch}");
        ios_println!(ios, "\n{}", text::display_url(url));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, graphql_repo_response, mock_graphql};

    #[tokio::test]
    async fn test_should_view_repository() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "repository",
            graphql_repo_response("owner", "repo"),
        )
        .await;

        let args = ViewArgs {
            repo: Some("owner/repo".into()),
            web: false,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("owner/repo"));
        assert!(out.contains("A test repository"));
        assert!(out.contains("public"));
        assert!(out.contains("Stars: 42"));
        assert!(out.contains("Forks: 5"));
        assert!(out.contains("Rust"));
    }

    #[tokio::test]
    async fn test_should_view_repository_in_browser() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            repo: Some("owner/repo".into()),
            web: true,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("owner/repo"));
    }

    #[tokio::test]
    async fn test_should_view_repository_json_output() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "repository",
            graphql_repo_response("owner", "repo"),
        )
        .await;

        let args = ViewArgs {
            repo: Some("owner/repo".into()),
            web: false,
            json: vec!["name".into()],
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("\"name\""));
        assert!(out.contains("\"repo\""));
    }

    #[tokio::test]
    async fn test_should_fail_without_repository_argument() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            repo: None,
            web: false,
            json: vec![],
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("required"));
    }
}
