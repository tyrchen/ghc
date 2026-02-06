//! `ghc repo clone` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;
use ghc_git::url_parser;

use crate::factory::Factory;

/// Clone a repository locally.
///
/// Pass additional `git clone` flags by listing them after `--`.
///
/// If the `OWNER/` portion of the `OWNER/REPO` repository argument is omitted,
/// it defaults to the name of the authenticating user.
///
/// If the repository is a fork, its parent repository will be added as an
/// additional git remote called `upstream`. The remote name can be configured
/// using `--upstream-remote-name`. The `--upstream-remote-name` option supports
/// an `@owner` value which will name the remote after the owner of the parent
/// repository.
#[derive(Debug, Args)]
pub struct CloneArgs {
    /// Repository to clone (OWNER/REPO or URL).
    #[arg(value_name = "REPOSITORY")]
    repo: String,

    /// Directory to clone into.
    #[arg(value_name = "DIRECTORY")]
    directory: Option<String>,

    /// Upstream remote name when cloning a fork.
    #[arg(short = 'u', long, default_value = "upstream")]
    upstream_remote_name: String,

    /// Additional git clone arguments.
    #[arg(last = true)]
    git_args: Vec<String>,
}

impl CloneArgs {
    /// Run the repo clone command.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        let git = factory.git_client()?;

        // Determine if input is a URL or OWNER/REPO
        let repo_is_url = self.repo.contains(':');
        let repo_is_full_name = !repo_is_url && self.repo.contains('/');

        let (repo, protocol) = if repo_is_url {
            // Parse as URL
            let parsed =
                url_parser::parse_url(&self.repo).context("failed to parse repository URL")?;
            let host = parsed.host_str().unwrap_or("github.com");
            let path = parsed.path().trim_matches('/');
            let path = path.trim_end_matches(".git");
            let parts: Vec<&str> = path.splitn(3, '/').collect();
            if parts.len() < 2 {
                anyhow::bail!("invalid repository URL: cannot extract owner/name");
            }

            let scheme = if parsed.scheme() == "git+ssh" {
                "ssh"
            } else {
                parsed.scheme()
            };
            (
                Repo::with_host(parts[0], parts[1], host),
                scheme.to_string(),
            )
        } else {
            let full_name = if repo_is_full_name {
                self.repo.clone()
            } else {
                // Default to current user's namespace
                let client = factory.api_client("github.com")?;
                let viewer: HashMap<String, Value> = client
                    .graphql(ghc_api::queries::user::VIEWER_QUERY, &HashMap::new())
                    .await
                    .context("failed to get authenticated user")?;
                let login = viewer
                    .get("viewer")
                    .and_then(|v| v.get("login"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("could not determine authenticated user"))?;
                format!("{login}/{}", self.repo)
            };

            let repo = Repo::from_full_name(&full_name)
                .context("invalid repository format, expected OWNER/REPO")?;

            let protocol = factory
                .config()
                .ok()
                .and_then(|c| {
                    let cfg = c.lock().ok()?;
                    Some(cfg.git_protocol(repo.host()))
                })
                .unwrap_or_else(|| "https".to_string());

            (repo, protocol)
        };

        // Check for wiki clone
        let wants_wiki = std::path::Path::new(repo.name())
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wiki"));
        let base_repo = if wants_wiki {
            let name = repo.name().trim_end_matches(".wiki");
            Repo::with_host(repo.owner(), name, repo.host())
        } else {
            repo.clone()
        };

        // Fetch canonical repo from API (for correct casing and fork detection)
        let client = factory.api_client(base_repo.host())?;
        let mut variables = HashMap::new();
        variables.insert(
            "owner".to_string(),
            Value::String(base_repo.owner().to_string()),
        );
        variables.insert(
            "name".to_string(),
            Value::String(base_repo.name().to_string()),
        );
        let data: Value = client
            .graphql(ghc_api::queries::repo::REPO_QUERY, &variables)
            .await
            .context("failed to fetch repository")?;

        let repo_data = data
            .get("repository")
            .ok_or_else(|| anyhow::anyhow!("repository not found: {}", base_repo.full_name()))?;

        let canonical_owner = repo_data
            .pointer("/owner/login")
            .and_then(Value::as_str)
            .unwrap_or(base_repo.owner());
        let canonical_name = repo_data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(base_repo.name());
        let canonical_repo = Repo::with_host(canonical_owner, canonical_name, base_repo.host());

        // Build clone URL
        let mut clone_url = url_parser::clone_url(&canonical_repo, &protocol);

        // Handle wiki clone
        if wants_wiki {
            let has_wiki = repo_data
                .get("hasWikiEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !has_wiki {
                anyhow::bail!(
                    "The '{}' repository does not have a wiki",
                    canonical_repo.full_name()
                );
            }
            clone_url = clone_url.trim_end_matches(".git").to_string() + ".wiki.git";
        }

        // Perform clone
        let git_arg_refs: Vec<&str> = self.git_args.iter().map(String::as_str).collect();
        let mut extra_args = git_arg_refs;
        if let Some(ref dir) = self.directory {
            extra_args.push(dir);
        }
        let clone_dir = git.clone(&clone_url, &extra_args).await?;

        // If repo is a fork, add parent as upstream remote
        if let Some(parent_data) = repo_data.get("parent")
            && !parent_data.is_null()
        {
            setup_upstream_remote(
                factory,
                parent_data,
                repo_data,
                &canonical_repo,
                &protocol,
                &self.upstream_remote_name,
                &clone_dir,
            )
            .await?;
        }

        Ok(())
    }
}

/// Set up the upstream remote for a forked repository after cloning.
async fn setup_upstream_remote(
    factory: &Factory,
    parent_data: &Value,
    repo_data: &Value,
    canonical_repo: &Repo,
    protocol: &str,
    upstream_remote_name: &str,
    clone_dir: &str,
) -> Result<()> {
    let parent_owner = parent_data
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parent_name = parent_data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");

    if parent_owner.is_empty() || parent_name.is_empty() {
        return Ok(());
    }

    let parent_repo = Repo::with_host(parent_owner, parent_name, canonical_repo.host());
    let upstream_url = url_parser::clone_url(&parent_repo, protocol);

    let upstream_name = if upstream_remote_name == "@owner" {
        parent_owner.to_string()
    } else {
        upstream_remote_name.to_string()
    };

    let default_branch = repo_data
        .pointer("/defaultBranchRef/name")
        .and_then(Value::as_str)
        .unwrap_or("main");

    let clone_git = ghc_git::client::GitClient::new()?.with_repo_dir(clone_dir);

    clone_git
        .add_remote(&upstream_name, &upstream_url, &[default_branch])
        .await
        .map_err(|e| anyhow::anyhow!("failed to add upstream remote '{upstream_name}': {e}"))?;

    clone_git
        .fetch(&upstream_name, "")
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch upstream remote '{upstream_name}': {e}"))?;

    clone_git
        .set_remote_branches(&upstream_name, "*")
        .await
        .map_err(|e| anyhow::anyhow!("failed to set remote branches: {e}"))?;

    clone_git
        .set_remote_resolution(&upstream_name, "base")
        .await
        .map_err(|e| anyhow::anyhow!("failed to set remote resolution: {e}"))?;

    let ios = &factory.io;
    if ios.is_stdout_tty() {
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Repository {} set as the default repository. \
             To learn more about the default repository, run: ghc repo set-default --help",
            cs.warning_icon(),
            cs.bold(&parent_repo.full_name()),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_construct_clone_args() {
        let args = CloneArgs {
            repo: "owner/repo".into(),
            directory: None,
            upstream_remote_name: "upstream".into(),
            git_args: vec![],
        };
        assert_eq!(args.repo, "owner/repo");
        assert_eq!(args.upstream_remote_name, "upstream");
    }

    #[test]
    fn test_should_detect_url_input() {
        let args = CloneArgs {
            repo: "https://github.com/cli/cli".into(),
            directory: None,
            upstream_remote_name: "upstream".into(),
            git_args: vec![],
        };
        assert!(args.repo.contains(':'));
    }

    #[test]
    fn test_should_accept_custom_upstream_name() {
        let args = CloneArgs {
            repo: "owner/repo".into(),
            directory: None,
            upstream_remote_name: "@owner".into(),
            git_args: vec![],
        };
        assert_eq!(args.upstream_remote_name, "@owner");
    }
}
