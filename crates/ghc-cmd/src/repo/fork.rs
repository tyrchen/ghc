//! `ghc repo fork` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Create a fork of a repository.
///
/// With no argument, creates a fork of the current repository. Otherwise, forks
/// the specified repository.
///
/// By default, the new fork is set to be your `origin` remote and any existing
/// origin remote is renamed to `upstream`. To alter this behavior, you can set
/// a name for the new fork's remote with `--remote-name`.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct ForkArgs {
    /// Repository to fork (OWNER/REPO or URL).
    #[arg(value_name = "REPOSITORY")]
    repository: Option<String>,

    /// Clone the fork.
    #[arg(long)]
    clone: Option<bool>,

    /// Add a git remote for the fork.
    #[arg(long)]
    remote: Option<bool>,

    /// Specify the name for the new remote.
    #[arg(long, default_value = "origin")]
    remote_name: String,

    /// Create the fork in an organization.
    #[arg(long)]
    org: Option<String>,

    /// Rename the forked repository.
    #[arg(long)]
    fork_name: Option<String>,

    /// Only include the default branch in the fork.
    #[arg(long)]
    default_branch_only: bool,
}

impl ForkArgs {
    /// Run the repo fork command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if let Some(ref org) = self.org
            && org.is_empty()
        {
            anyhow::bail!("--org cannot be blank");
        }

        if self.remote_name.is_empty() {
            anyhow::bail!("--remote-name cannot be blank");
        }

        let repo = match &self.repository {
            Some(r) => parse_repo_arg(r)?,
            None => {
                anyhow::bail!("repository argument required (e.g. OWNER/REPO)")
            }
        };

        let client = factory.api_client(repo.host())?;
        let forked = self.create_fork(&client, &repo).await?;
        let connected_to_terminal = ios.is_stdout_tty() && ios.is_stderr_tty();

        let forked_full_name = forked
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let forked_name = forked.get("name").and_then(Value::as_str).unwrap_or("");

        let already_existed = check_already_existed(&forked);

        print_fork_status(
            ios,
            &cs,
            forked_full_name,
            &forked,
            already_existed,
            connected_to_terminal,
        );
        self.rename_if_needed(
            &client,
            forked_name,
            &forked,
            ios,
            &cs,
            connected_to_terminal,
        )
        .await?;
        self.clone_if_requested(factory, &forked, &repo, ios, &cs, connected_to_terminal)
            .await?;

        Ok(())
    }

    async fn create_fork(&self, client: &ghc_api::client::Client, repo: &Repo) -> Result<Value> {
        let mut fork_body = serde_json::json!({
            "default_branch_only": self.default_branch_only,
        });
        if let Some(ref org) = self.org {
            fork_body["organization"] = Value::String(org.clone());
        }
        if let Some(ref name) = self.fork_name {
            fork_body["name"] = Value::String(name.clone());
        }

        let fork_path = format!("repos/{}/{}/forks", repo.owner(), repo.name());
        client
            .rest(reqwest::Method::POST, &fork_path, Some(&fork_body))
            .await
            .context("failed to fork repository")
    }

    async fn rename_if_needed(
        &self,
        client: &ghc_api::client::Client,
        forked_name: &str,
        forked: &Value,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
        connected_to_terminal: bool,
    ) -> Result<()> {
        let Some(ref desired_name) = self.fork_name else {
            return Ok(());
        };
        let normalized = normalize_repo_name(desired_name);
        if forked_name.eq_ignore_ascii_case(&normalized) {
            return Ok(());
        }

        let forked_owner = forked
            .pointer("/owner/login")
            .and_then(Value::as_str)
            .unwrap_or("");
        let rename_path = format!("repos/{forked_owner}/{forked_name}");
        let rename_body = serde_json::json!({ "name": desired_name });
        let renamed: Value = client
            .rest(reqwest::Method::PATCH, &rename_path, Some(&rename_body))
            .await
            .context("could not rename fork")?;

        let renamed_full = renamed
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if connected_to_terminal {
            ios_eprintln!(
                ios,
                "{} Renamed fork to {}",
                cs.success_icon(),
                cs.bold(renamed_full)
            );
        }
        Ok(())
    }

    async fn clone_if_requested(
        &self,
        factory: &crate::factory::Factory,
        forked: &Value,
        repo: &Repo,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
        connected_to_terminal: bool,
    ) -> Result<()> {
        let should_clone = match self.clone {
            Some(v) => v,
            None => {
                if ios.can_prompt() {
                    factory
                        .prompter()
                        .confirm("Would you like to clone the fork?", false)?
                } else {
                    false
                }
            }
        };

        if !should_clone {
            return Ok(());
        }

        let clone_url = forked
            .get("clone_url")
            .and_then(Value::as_str)
            .unwrap_or("");

        if clone_url.is_empty() {
            return Ok(());
        }

        let git_client = factory.git_client()?;
        let clone_dir = git_client
            .clone(clone_url, &[])
            .await
            .context("failed to clone fork")?;

        let upstream_url = format!("https://github.com/{}/{}.git", repo.owner(), repo.name());
        let cloned_git = ghc_git::client::GitClient::new()?.with_repo_dir(&clone_dir);
        cloned_git
            .add_remote("upstream", &upstream_url, &[])
            .await
            .context("failed to add upstream remote")?;
        cloned_git
            .set_remote_resolution("upstream", "base")
            .await
            .context("failed to set upstream resolution")?;
        cloned_git
            .fetch("upstream", "")
            .await
            .context("failed to fetch upstream")?;

        if connected_to_terminal {
            ios_eprintln!(ios, "{} Cloned fork", cs.success_icon());
        }

        Ok(())
    }
}

/// Print the status of the fork operation (created or already existed).
fn print_fork_status(
    ios: &ghc_core::iostreams::IOStreams,
    cs: &ghc_core::iostreams::ColorScheme,
    forked_full_name: &str,
    forked: &Value,
    already_existed: bool,
    connected_to_terminal: bool,
) {
    if already_existed {
        if connected_to_terminal {
            ios_eprintln!(
                ios,
                "{} {} already exists",
                cs.warning("!"),
                cs.bold(forked_full_name)
            );
        } else {
            ios_eprintln!(ios, "{forked_full_name} already exists");
        }
    } else if connected_to_terminal {
        ios_eprintln!(
            ios,
            "{} Created fork {}",
            cs.success_icon(),
            cs.bold(forked_full_name)
        );
    } else {
        let html_url = forked.get("html_url").and_then(Value::as_str).unwrap_or("");
        ios_println!(ios, "{html_url}");
    }
}

/// Check if a fork already existed by looking at its creation timestamp.
fn check_already_existed(forked: &Value) -> bool {
    let Some(created_at) = forked.get("created_at").and_then(Value::as_str) else {
        return false;
    };
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return false;
    };
    let now = chrono::Utc::now();
    let age = now.signed_duration_since(created);
    age.num_seconds() > 60
}

/// Parse a repository argument that could be OWNER/REPO or a URL.
fn parse_repo_arg(arg: &str) -> Result<Repo> {
    if arg.starts_with("http:/") || arg.starts_with("https:/") {
        let parsed = url::Url::parse(arg).context("did not understand argument")?;
        Repo::from_url(&parsed).map_err(|e| anyhow::anyhow!("did not understand argument: {e}"))
    } else if arg.starts_with("git@") {
        // Convert git@host:owner/repo to https://host/owner/repo for parsing
        let rest = arg.strip_prefix("git@").unwrap_or(arg);
        let normalized = rest.replacen(':', "/", 1);
        let url_str = format!("https://{normalized}");
        let parsed = url::Url::parse(&url_str).context("did not understand argument")?;
        Repo::from_url(&parsed).map_err(|e| anyhow::anyhow!("did not understand argument: {e}"))
    } else {
        Repo::from_full_name(arg).map_err(|e| anyhow::anyhow!("argument error: {e}"))
    }
}

/// Normalize a repository name using the same logic as GitHub.
fn normalize_repo_name(name: &str) -> String {
    let re = regex::Regex::new(r"[^\w._-]+")
        .unwrap_or_else(|_| regex::Regex::new(".").expect("valid regex"));
    let normalized = re.replace_all(name, "-");
    normalized.trim_end_matches(".git").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_fork_repository() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/forks",
            202,
            serde_json::json!({
                "full_name": "testuser/repo",
                "name": "repo",
                "html_url": "https://github.com/testuser/repo",
                "clone_url": "https://github.com/testuser/repo.git",
                "owner": { "login": "testuser" },
                "created_at": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .await;

        let args = ForkArgs {
            repository: Some("owner/repo".into()),
            clone: Some(false),
            remote: None,
            remote_name: "origin".into(),
            org: None,
            fork_name: None,
            default_branch_only: false,
        };
        // Succeeds without error (TTY stderr output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }

    #[tokio::test]
    async fn test_should_detect_existing_fork() {
        let h = TestHarness::new().await;
        let old_time = chrono::Utc::now() - chrono::Duration::hours(1);
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/forks",
            202,
            serde_json::json!({
                "full_name": "testuser/repo",
                "name": "repo",
                "html_url": "https://github.com/testuser/repo",
                "clone_url": "https://github.com/testuser/repo.git",
                "owner": { "login": "testuser" },
                "created_at": old_time.to_rfc3339(),
            }),
        )
        .await;

        let args = ForkArgs {
            repository: Some("owner/repo".into()),
            clone: Some(false),
            remote: None,
            remote_name: "origin".into(),
            org: None,
            fork_name: None,
            default_branch_only: false,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn test_should_parse_repo_arg_owner_repo() {
        let repo = parse_repo_arg("cli/cli").unwrap();
        assert_eq!(repo.owner(), "cli");
        assert_eq!(repo.name(), "cli");
    }

    #[test]
    fn test_should_parse_repo_arg_url() {
        let repo = parse_repo_arg("https://github.com/cli/cli").unwrap();
        assert_eq!(repo.owner(), "cli");
        assert_eq!(repo.name(), "cli");
    }

    #[test]
    fn test_should_parse_repo_arg_ssh() {
        let repo = parse_repo_arg("git@github.com:cli/cli.git").unwrap();
        assert_eq!(repo.owner(), "cli");
        assert_eq!(repo.name(), "cli");
    }

    #[test]
    fn test_should_normalize_repo_name() {
        assert_eq!(normalize_repo_name("my repo"), "my-repo");
        assert_eq!(normalize_repo_name("repo.git"), "repo");
        assert_eq!(normalize_repo_name("valid-name"), "valid-name");
    }
}
