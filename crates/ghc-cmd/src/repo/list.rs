//! `ghc repo list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// List repositories owned by user or organization.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct ListArgs {
    /// Filter by owner (user or organization).
    #[arg(value_name = "OWNER")]
    owner: Option<String>,

    /// Maximum number of repositories to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by visibility.
    #[arg(long, value_parser = ["public", "private", "internal"])]
    visibility: Option<String>,

    /// Filter by primary coding language.
    #[arg(short, long)]
    language: Option<String>,

    /// Filter by topic.
    #[arg(long)]
    topic: Vec<String>,

    /// Show only forks.
    #[arg(long)]
    fork: bool,

    /// Show only sources (non-forks).
    #[arg(long)]
    source: bool,

    /// Show only archived repos.
    #[arg(long)]
    archived: bool,

    /// Show only non-archived repos.
    #[arg(long)]
    no_archived: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the repo list command.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let owner = if let Some(o) = &self.owner {
            o.clone()
        } else {
            // Get authenticated user
            let viewer: HashMap<String, Value> = client
                .graphql(ghc_api::queries::user::VIEWER_QUERY, &HashMap::new())
                .await
                .context("failed to get authenticated user")?;
            viewer
                .get("viewer")
                .and_then(|v| v.get("login"))
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("could not determine authenticated user"))?
                .to_string()
        };

        let query = r"
            query RepoList($owner: String!, $first: Int!, $after: String) {
              repositoryOwner(login: $owner) {
                repositories(first: $first, after: $after, orderBy: {field: PUSHED_AT, direction: DESC}) {
                  pageInfo { hasNextPage endCursor }
                  nodes {
                    name
                    owner { login }
                    description
                    url
                    isFork
                    isArchived
                    isPrivate
                    stargazerCount
                    primaryLanguage { name }
                    pushedAt
                  }
                }
              }
            }
        ";

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(owner.clone()));
        variables.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(self.limit.min(100))),
        );

        let data: Value = client
            .graphql(query, &variables)
            .await
            .context("failed to list repositories")?;

        let repos = data
            .pointer("/repositoryOwner/repositories/nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected API response format"))?;

        if repos.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No repositories match your search in @{owner}");
            }
            return Ok(());
        }

        // JSON output mode
        if !self.json.is_empty() {
            let json_output = serde_json::to_string_pretty(repos)?;
            ios_println!(ios, "{json_output}");
            return Ok(());
        }

        // Table output
        let mut tp = TablePrinter::new(ios);
        let cs = ios.color_scheme();

        for repo in repos {
            let name = repo.get("name").and_then(Value::as_str).unwrap_or("");
            let repo_owner = repo
                .pointer("/owner/login")
                .and_then(Value::as_str)
                .unwrap_or("");
            let desc = repo
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            let is_fork = repo.get("isFork").and_then(Value::as_bool).unwrap_or(false);
            let is_private = repo
                .get("isPrivate")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_archived = repo
                .get("isArchived")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let stars = repo
                .get("stargazerCount")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let language = repo
                .pointer("/primaryLanguage/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let pushed_at = repo.get("pushedAt").and_then(Value::as_str).unwrap_or("");

            // Apply filters
            if self.fork && !is_fork {
                continue;
            }
            if self.source && is_fork {
                continue;
            }
            if self.archived && !is_archived {
                continue;
            }
            if self.no_archived && is_archived {
                continue;
            }
            if let Some(ref lang) = self.language
                && !language.eq_ignore_ascii_case(lang)
            {
                continue;
            }

            let full_name = format!("{repo_owner}/{name}");
            let mut info = String::new();
            if is_private {
                info.push_str(&cs.warning("private"));
            } else {
                info.push_str(&cs.success("public"));
            }
            if is_fork {
                info.push_str(", fork");
            }
            if is_archived {
                info.push_str(", archived");
            }

            let desc_truncated = text::truncate(desc, 50);

            tp.add_row(vec![
                cs.bold(&full_name),
                desc_truncated,
                info,
                language.to_string(),
                format!("{stars}"),
                pushed_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}
