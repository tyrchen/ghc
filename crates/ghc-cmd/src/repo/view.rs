//! `ghc repo view` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::text;

use crate::factory::Factory;

/// View a repository.
///
/// Display the description and the README of a GitHub repository.
///
/// With `--web`, open the repository in a web browser instead.
///
/// With `--branch`, view a specific branch of the repository.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Repository to view (OWNER/REPO).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// Open in web browser.
    #[arg(short, long)]
    web: bool,

    /// View a specific branch of the repository.
    #[arg(short, long)]
    branch: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

/// README content fetched from the REST API.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ReadmeResponse {
    name: String,
    content: String,
    html_url: String,
}

impl ViewArgs {
    /// Run the repo view command.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        let repo = match &self.repo {
            Some(r) => {
                ghc_core::repo::Repo::from_full_name(r).context("invalid repository format")?
            }
            None => {
                anyhow::bail!("repository argument required (e.g. OWNER/REPO)")
            }
        };

        // Build URL with optional branch
        let open_url = if let Some(ref branch) = self.branch {
            format!(
                "https://{}/{}/{}/tree/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                urlencoding::encode(branch),
            )
        } else {
            format!("https://{}/{}/{}", repo.host(), repo.owner(), repo.name())
        };

        if self.web {
            if factory.io.is_stdout_tty() {
                ghc_core::ios_eprintln!(
                    factory.io,
                    "Opening {} in your browser.",
                    text::display_url(&open_url),
                );
            }
            factory.browser().open(&open_url)?;
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

        // Fetch README via REST API
        let readme_content = self.fetch_readme(&client, &repo).await;

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

        if !ios.is_stdout_tty() {
            // Machine-readable output (non-TTY)
            ios_println!(ios, "name:\t{owner_login}/{name}");
            ios_println!(ios, "description:\t{description}");
            if let Some(ref content) = readme_content {
                ios_println!(ios, "--");
                ios_println!(ios, "{content}");
            }
            return Ok(());
        }

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

        // Display README
        if let Some(ref content) = readme_content {
            ios_println!(ios, "");
            ios_println!(ios, "{content}");
        } else {
            ios_println!(ios, "");
            ios_println!(ios, "{}", cs.gray("This repository does not have a README"));
        }

        ios_println!(ios, "\n{}", text::display_url(url));

        Ok(())
    }

    /// Fetch the README from the REST API.
    async fn fetch_readme(
        &self,
        client: &ghc_api::client::Client,
        repo: &ghc_core::repo::Repo,
    ) -> Option<String> {
        let mut path = format!("repos/{}/{}/readme", repo.owner(), repo.name());
        if let Some(ref branch) = self.branch {
            path = format!("{path}?ref={branch}");
        }

        let response: Result<ReadmeResponse, _> =
            client.rest(reqwest::Method::GET, &path, None).await;

        match response {
            Ok(readme) => {
                // README content is base64-encoded
                let bytes =
                    ghc_core::text::base64_decode(&readme.content.replace('\n', "")).ok()?;
                let text = String::from_utf8(bytes).ok()?;
                if text.is_empty() { None } else { Some(text) }
            }
            Err(_) => None,
        }
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
            branch: None,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        // Non-TTY mode outputs machine-readable format
        let out = h.stdout();
        assert!(
            out.contains("owner/repo"),
            "should contain repo name: {out}"
        );
        assert!(
            out.contains("A test repository"),
            "should contain description: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_view_repository_in_browser() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            repo: Some("owner/repo".into()),
            web: true,
            branch: None,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("owner/repo"));
    }

    #[tokio::test]
    async fn test_should_view_repository_in_browser_with_branch() {
        let h = TestHarness::new().await;

        let args = ViewArgs {
            repo: Some("owner/repo".into()),
            web: true,
            branch: Some("develop".into()),
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("owner/repo/tree/develop"));
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
            branch: None,
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
            branch: None,
            json: vec![],
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("required"));
    }
}
