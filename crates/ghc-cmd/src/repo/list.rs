//! `ghc repo list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

use crate::factory::Factory;

/// List repositories owned by user or organization.
///
/// Note that the list will only include repositories owned by the provided
/// argument, and the `--fork` or `--source` flags will not traverse ownership
/// boundaries.
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

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

/// Result of listing repositories, including total count and ownership info.
#[derive(Debug)]
struct RepoListResult {
    owner: String,
    repos: Vec<Value>,
    total_count: i64,
    from_search: bool,
}

impl ListArgs {
    /// Run the repo list command.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        // Validate mutually exclusive flags
        if self.source && self.fork {
            anyhow::bail!("specify only one of `--source` or `--fork`");
        }
        if self.archived && self.no_archived {
            anyhow::bail!("specify only one of `--archived` or `--no-archived`");
        }
        if self.limit < 1 {
            anyhow::bail!("invalid limit: {}", self.limit);
        }

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

        // Use the Search API when filters require it (language, archived, topic, internal)
        let needs_search = self.language.is_some()
            || self.archived
            || self.no_archived
            || !self.topic.is_empty()
            || self.visibility.as_deref() == Some("internal");

        let result = if needs_search {
            self.search_repos(&client, &owner).await?
        } else {
            self.list_repos(&client, &owner).await?
        };

        // Handle case where owner is not found
        if self.owner.is_some() && result.owner.is_empty() && !result.from_search {
            anyhow::bail!(
                "the owner handle {owner:?} was not recognized as either a GitHub user or an organization"
            );
        }

        // JSON output mode with field filtering, jq, or template
        // Always produces output (even [] for empty results)
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let mut arr = Value::Array(result.repos.clone());
            ghc_core::json::normalize_graphql_connections(&mut arr);
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if result.repos.is_empty() {
            if self.has_filters() {
                ios_eprintln!(ios, "No results match your search");
            } else if !owner.is_empty() {
                ios_eprintln!(ios, "There are no repositories in @{owner}");
            } else {
                ios_eprintln!(ios, "No results");
            }
            return Ok(());
        }

        // Search API warning
        if result.from_search && self.limit > 1000 {
            ios_eprintln!(
                ios,
                "warning: this query uses the Search API which is capped at 1000 results maximum"
            );
        }

        // Header (TTY only)
        let match_count = result.repos.len();
        if ios.is_stdout_tty() {
            let header = self.list_header(&result.owner, match_count, result.total_count);
            ios_println!(ios, "\n{header}\n");
        }

        // Table output
        let mut tp = TablePrinter::new(ios);
        let cs = ios.color_scheme();

        for repo in &result.repos {
            let name_with_owner = repo
                .get("nameWithOwner")
                .and_then(Value::as_str)
                .map_or_else(
                    || {
                        let name = repo.get("name").and_then(Value::as_str).unwrap_or("");
                        let repo_owner = repo
                            .pointer("/owner/login")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        format!("{repo_owner}/{name}")
                    },
                    String::from,
                );

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
            let vis_field = repo.get("visibility").and_then(Value::as_str).unwrap_or("");
            let pushed_at = repo.get("pushedAt").and_then(Value::as_str).unwrap_or("");
            let created_at = repo.get("createdAt").and_then(Value::as_str).unwrap_or("");

            // Build info string
            let vis_label = if !vis_field.is_empty() {
                vis_field.to_lowercase()
            } else if is_private {
                "private".to_string()
            } else {
                "public".to_string()
            };
            let mut info_parts = vec![vis_label];
            if is_fork {
                info_parts.push("fork".to_string());
            }
            if is_archived {
                info_parts.push("archived".to_string());
            }
            let info = info_parts.join(", ");

            // Format time -- prefer pushedAt, fall back to createdAt
            let time_str = if pushed_at.is_empty() {
                created_at
            } else {
                pushed_at
            };
            let updated = if time_str.is_empty() {
                String::new()
            } else {
                chrono::DateTime::parse_from_rfc3339(time_str).map_or_else(
                    |_| time_str.to_string(),
                    |dt| {
                        let duration = chrono::Utc::now().signed_duration_since(dt);
                        text::fuzzy_ago(duration)
                    },
                )
            };

            let desc_clean = text::remove_excessive_whitespace(desc);
            let desc_truncated = text::truncate(&desc_clean, 50);

            tp.add_row(vec![
                cs.bold(&name_with_owner),
                desc_truncated,
                info,
                updated,
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }

    /// List repos using the GraphQL `repositoryOwner` query with pagination.
    #[allow(clippy::too_many_lines)]
    async fn list_repos(
        &self,
        client: &ghc_api::client::Client,
        owner: &str,
    ) -> Result<RepoListResult> {
        let query = r"
            query RepoList($owner: String!, $first: Int!, $after: String, $privacy: RepositoryPrivacy, $fork: Boolean) {
              repositoryOwner(login: $owner) {
                login
                repositories(first: $first, after: $after, privacy: $privacy, isFork: $fork, ownerAffiliations: OWNER, orderBy: {field: PUSHED_AT, direction: DESC}) {
                  totalCount
                  pageInfo { hasNextPage endCursor }
                  nodes {
                    name
                    nameWithOwner
                    owner { login }
                    description
                    url
                    isFork
                    isArchived
                    isPrivate
                    visibility
                    stargazerCount
                    primaryLanguage { name }
                    pushedAt
                    createdAt
                  }
                }
              }
            }
        ";

        let per_page = self.limit.min(100);
        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(owner.to_string()));
        variables.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(per_page)),
        );

        // Apply visibility filter at the API level
        if let Some(ref vis) = self.visibility {
            variables.insert("privacy".to_string(), Value::String(vis.to_uppercase()));
        }

        // Apply fork filter at the API level
        if self.fork {
            variables.insert("fork".to_string(), Value::Bool(true));
        } else if self.source {
            variables.insert("fork".to_string(), Value::Bool(false));
        }

        let mut result = RepoListResult {
            owner: String::new(),
            repos: Vec::new(),
            total_count: 0,
            from_search: false,
        };

        loop {
            let data: Value = client
                .graphql(query, &variables)
                .await
                .context("failed to list repositories")?;

            let repo_owner_data = data.get("repositoryOwner");
            if repo_owner_data.is_none() || repo_owner_data.is_some_and(Value::is_null) {
                // Owner not found
                return Ok(result);
            }
            let repo_owner_data = repo_owner_data.unwrap();

            if result.owner.is_empty() {
                result.owner = repo_owner_data
                    .get("login")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
            }

            let Some(repos_data) = repo_owner_data.get("repositories") else {
                break;
            };

            result.total_count = repos_data
                .get("totalCount")
                .and_then(Value::as_i64)
                .unwrap_or(0);

            let nodes = repos_data
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for node in nodes {
                result.repos.push(node);
                if result.repos.len() >= self.limit as usize {
                    return Ok(result);
                }
            }

            // Check pagination
            let has_next = repos_data
                .pointer("/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_next {
                break;
            }

            let end_cursor = repos_data
                .pointer("/pageInfo/endCursor")
                .and_then(Value::as_str);
            match end_cursor {
                Some(cursor) => {
                    variables.insert("after".to_string(), Value::String(cursor.to_string()));
                }
                None => break,
            }
        }

        Ok(result)
    }

    /// Search repos using the GraphQL `search` API for advanced filters.
    async fn search_repos(
        &self,
        client: &ghc_api::client::Client,
        owner: &str,
    ) -> Result<RepoListResult> {
        let query = r"
            query RepositoryListSearch($query: String!, $first: Int!, $after: String) {
              search(type: REPOSITORY, query: $query, first: $first, after: $after) {
                repositoryCount
                nodes {
                  ... on Repository {
                    name
                    nameWithOwner
                    owner { login }
                    description
                    url
                    isFork
                    isArchived
                    isPrivate
                    visibility
                    stargazerCount
                    primaryLanguage { name }
                    pushedAt
                    createdAt
                  }
                }
                pageInfo { hasNextPage endCursor }
              }
            }
        ";

        let search_query = self.build_search_query(owner);
        let per_page = self.limit.min(100);

        let mut variables = HashMap::new();
        variables.insert("query".to_string(), Value::String(search_query));
        variables.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(per_page)),
        );

        let mut result = RepoListResult {
            owner: owner.to_string(),
            repos: Vec::new(),
            total_count: 0,
            from_search: true,
        };

        loop {
            let data: Value = client
                .graphql(query, &variables)
                .await
                .context("failed to search repositories")?;

            let search_data = data
                .get("search")
                .ok_or_else(|| anyhow::anyhow!("unexpected search API response"))?;

            result.total_count = search_data
                .get("repositoryCount")
                .and_then(Value::as_i64)
                .unwrap_or(0);

            let nodes = search_data
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            for node in nodes {
                // Extract owner from nameWithOwner if not set
                if result.owner.is_empty()
                    && let Some(nwo) = node.get("nameWithOwner").and_then(Value::as_str)
                    && let Some(idx) = nwo.find('/')
                {
                    result.owner = nwo[..idx].to_string();
                }
                result.repos.push(node);
                if result.repos.len() >= self.limit as usize {
                    return Ok(result);
                }
            }

            // Check pagination
            let has_next = search_data
                .pointer("/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_next {
                break;
            }

            let end_cursor = search_data
                .pointer("/pageInfo/endCursor")
                .and_then(Value::as_str);
            match end_cursor {
                Some(cursor) => {
                    variables.insert("after".to_string(), Value::String(cursor.to_string()));
                }
                None => break,
            }
        }

        Ok(result)
    }

    /// Build a GitHub search query string for repository search.
    fn build_search_query(&self, owner: &str) -> String {
        let mut parts = Vec::new();

        // Owner qualifier
        let user = if owner.is_empty() { "@me" } else { owner };
        parts.push(format!("user:{user}"));

        // Sort
        parts.push("sort:updated-desc".to_string());

        // Fork filter
        if self.fork {
            parts.push("fork:only".to_string());
        } else if self.source {
            parts.push("fork:false".to_string());
        } else {
            parts.push("fork:true".to_string());
        }

        // Visibility
        if let Some(ref vis) = self.visibility {
            parts.push(format!("is:{vis}"));
        }

        // Language
        if let Some(ref lang) = self.language {
            parts.push(format!("language:{lang}"));
        }

        // Topics
        for topic in &self.topic {
            parts.push(format!("topic:{topic}"));
        }

        // Archived
        if self.archived {
            parts.push("archived:true".to_string());
        } else if self.no_archived {
            parts.push("archived:false".to_string());
        }

        parts.join(" ")
    }

    /// Build the header line for TTY output.
    fn list_header(&self, owner: &str, match_count: usize, total_count: i64) -> String {
        if total_count == 0 {
            if self.has_filters() {
                return "No results match your search".to_string();
            }
            if !owner.is_empty() {
                return format!("There are no repositories in @{owner}");
            }
            return "No results".to_string();
        }

        let filter_str = if self.has_filters() {
            " that match your search"
        } else {
            ""
        };
        format!("Showing {match_count} of {total_count} repositories in @{owner}{filter_str}")
    }

    /// Check whether any filter flags are set.
    fn has_filters(&self) -> bool {
        self.visibility.is_some()
            || self.fork
            || self.source
            || self.language.is_some()
            || !self.topic.is_empty()
            || self.archived
            || self.no_archived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql};

    fn repo_list_response(repos: &[Value]) -> Value {
        serde_json::json!({
            "data": {
                "repositoryOwner": {
                    "login": "testuser",
                    "repositories": {
                        "totalCount": repos.len(),
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        },
                        "nodes": repos
                    }
                }
            }
        })
    }

    fn repo_fixture(name: &str, is_private: bool, is_fork: bool) -> Value {
        serde_json::json!({
            "name": name,
            "nameWithOwner": format!("testuser/{name}"),
            "owner": { "login": "testuser" },
            "description": format!("Description of {name}"),
            "url": format!("https://github.com/testuser/{name}"),
            "isFork": is_fork,
            "isArchived": false,
            "isPrivate": is_private,
            "visibility": if is_private { "PRIVATE" } else { "PUBLIC" },
            "stargazerCount": 10,
            "primaryLanguage": { "name": "Rust" },
            "pushedAt": "2024-06-15T10:00:00Z",
            "createdAt": "2024-01-01T00:00:00Z"
        })
    }

    fn viewer_response() -> Value {
        serde_json::json!({
            "data": {
                "viewer": {
                    "login": "testuser"
                }
            }
        })
    }

    #[tokio::test]
    async fn test_should_list_repositories() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "viewer", viewer_response()).await;
        mock_graphql(
            &h.server,
            "RepoList",
            repo_list_response(&[
                repo_fixture("alpha", false, false),
                repo_fixture("beta", true, false),
            ]),
        )
        .await;

        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("testuser/alpha"));
        assert!(out.contains("testuser/beta"));
    }

    #[tokio::test]
    async fn test_should_list_with_owner() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "RepoList",
            repo_list_response(&[repo_fixture("project", false, false)]),
        )
        .await;

        let args = ListArgs {
            owner: Some("someorg".into()),
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("testuser/project"));
    }

    #[tokio::test]
    async fn test_should_show_empty_message() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "viewer", viewer_response()).await;
        mock_graphql(&h.server, "RepoList", repo_list_response(&[])).await;

        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("no repositories")
                || err.contains("No results")
                || err.contains("There are no repositories")
        );
    }

    #[tokio::test]
    async fn test_should_use_search_api_for_language_filter() {
        let h = TestHarness::new().await;
        mock_graphql(&h.server, "viewer", viewer_response()).await;
        mock_graphql(
            &h.server,
            "RepositoryListSearch",
            serde_json::json!({
                "data": {
                    "search": {
                        "repositoryCount": 1,
                        "nodes": [repo_fixture("rust-proj", false, false)],
                        "pageInfo": {
                            "hasNextPage": false,
                            "endCursor": null
                        }
                    }
                }
            }),
        )
        .await;

        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: Some("Rust".into()),
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("testuser/rust-proj"));
    }

    #[tokio::test]
    async fn test_should_reject_conflicting_flags() {
        let h = TestHarness::new().await;

        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: true,
            source: true,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("source"));
    }

    #[tokio::test]
    async fn test_should_reject_conflicting_archived_flags() {
        let h = TestHarness::new().await;

        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: false,
            source: false,
            archived: true,
            no_archived: true,
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("archived"));
    }

    #[test]
    fn test_should_build_search_query() {
        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: Some("private".into()),
            language: Some("Rust".into()),
            topic: vec!["cli".into()],
            fork: false,
            source: true,
            archived: false,
            no_archived: true,
            json: vec![],
            jq: None,
            template: None,
        };

        let query = args.build_search_query("myuser");
        assert!(query.contains("user:myuser"));
        assert!(query.contains("fork:false"));
        assert!(query.contains("is:private"));
        assert!(query.contains("language:Rust"));
        assert!(query.contains("topic:cli"));
        assert!(query.contains("archived:false"));
    }

    #[test]
    fn test_should_build_search_query_fork_only() {
        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: true,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let query = args.build_search_query("org");
        assert!(query.contains("fork:only"));
    }

    #[test]
    fn test_should_format_list_header() {
        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: None,
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let header = args.list_header("testuser", 5, 42);
        assert_eq!(header, "Showing 5 of 42 repositories in @testuser");
    }

    #[test]
    fn test_should_format_list_header_with_filters() {
        let args = ListArgs {
            owner: None,
            limit: 30,
            visibility: None,
            language: Some("Rust".into()),
            topic: vec![],
            fork: false,
            source: false,
            archived: false,
            no_archived: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let header = args.list_header("testuser", 3, 100);
        assert!(header.contains("that match your search"));
    }
}
