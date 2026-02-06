//! `ghc repo sync` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// Sync a repository.
///
/// Sync destination repository from source repository. Syncing uses the default
/// branch of the source repository to update the matching branch on the
/// destination repository so they are equal. A fast forward update will be used
/// except when the `--force` flag is specified, then the two branches will be
/// synced using a hard reset.
///
/// Without an argument, the local repository is selected as the destination
/// repository.
///
/// The source repository is the parent of the destination repository by default.
/// This can be overridden with the `--source` flag.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Destination repository (OWNER/REPO). If omitted, syncs the local repo.
    #[arg(value_name = "DESTINATION")]
    destination: Option<String>,

    /// Source repository (OWNER/REPO).
    #[arg(short, long)]
    source: Option<String>,

    /// Branch to sync (default: the default branch).
    #[arg(short, long)]
    branch: Option<String>,

    /// Hard reset the branch of the destination repository to match the source.
    #[arg(long)]
    force: bool,
}

impl SyncArgs {
    /// Run the repo sync command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        if self.destination.is_some() {
            self.sync_remote(factory).await
        } else {
            self.sync_local(factory).await
        }
    }

    /// Sync the local repository from a remote source.
    async fn sync_local(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let src_repo = if let Some(s) = &self.source {
            Repo::from_full_name(s).context("invalid source repository")?
        } else {
            anyhow::bail!("source repository required for local sync (use --source OWNER/REPO)")
        };

        let client = factory.api_client(src_repo.host())?;
        let git_client = factory.git_client()?;

        let remote_name = find_remote_name(git_client, &src_repo).await?;
        let branch = self
            .resolve_branch(&client, src_repo.owner(), src_repo.name())
            .await?;

        git_client
            .fetch(&remote_name, &format!("refs/heads/{branch}"))
            .await
            .context("failed to fetch from remote")?;

        self.apply_local_sync(git_client, &branch, &remote_name)
            .await?;

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Synced the \"{}\" branch from \"{}\" to local repository",
                cs.success_icon(),
                branch,
                src_repo.full_name()
            );
        }

        Ok(())
    }

    async fn apply_local_sync(
        &self,
        git_client: &ghc_git::client::GitClient,
        branch: &str,
        remote_name: &str,
    ) -> Result<()> {
        let has_local = git_client.has_local_branch(branch).await;

        if has_local && !self.force {
            let is_ff = git_client.run_merge_base_check(branch, "FETCH_HEAD").await;
            if !is_ff {
                anyhow::bail!(
                    "can't sync because there are diverging changes; use `--force` to overwrite the destination branch"
                );
            }
        }

        let current_branch = git_client.current_branch().await.unwrap_or_default();

        if current_branch == branch {
            let change_count = git_client.uncommitted_change_count().await.unwrap_or(0);
            if change_count > 0 {
                anyhow::bail!(
                    "refusing to sync due to uncommitted/untracked local changes\n\
                     tip: use `git stash --all` before retrying the sync and run `git stash pop` afterwards"
                );
            }

            if self.force {
                git_client
                    .reset_hard("FETCH_HEAD")
                    .await
                    .context("failed to reset to FETCH_HEAD")?;
            } else {
                git_client
                    .merge("FETCH_HEAD", true)
                    .await
                    .context("failed to fast-forward merge")?;
            }
        } else if has_local {
            git_client
                .update_ref(branch, "FETCH_HEAD")
                .await
                .context("failed to update branch ref")?;
        } else {
            git_client
                .create_branch_from(branch, "FETCH_HEAD", &format!("{remote_name}/{branch}"))
                .await
                .context("failed to create branch")?;
        }

        Ok(())
    }

    /// Sync a remote fork from its parent or from a specified source.
    async fn sync_remote(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let dest = self.destination.as_deref().unwrap_or_default();
        let dest_repo = Repo::from_full_name(dest).context("invalid destination repository")?;

        let client = factory.api_client(dest_repo.host())?;
        let branch = self
            .resolve_branch(&client, dest_repo.owner(), dest_repo.name())
            .await?;

        // Try merge-upstream API first
        if let Some(()) = self
            .try_merge_upstream(&client, &dest_repo, &branch, ios, &cs)
            .await?
        {
            return Ok(());
        }

        self.sync_via_git_refs(&client, &dest_repo, &branch, ios, &cs)
            .await
    }

    async fn resolve_branch(
        &self,
        client: &ghc_api::client::Client,
        owner: &str,
        name: &str,
    ) -> Result<String> {
        if let Some(b) = &self.branch {
            return Ok(b.clone());
        }

        let mut variables = HashMap::new();
        variables.insert("owner".into(), Value::String(owner.to_string()));
        variables.insert("name".into(), Value::String(name.to_string()));

        let data: Value = client
            .graphql(DEFAULT_BRANCH_QUERY, &variables)
            .await
            .context("failed to fetch default branch")?;

        Ok(data
            .pointer("/repository/defaultBranchRef/name")
            .and_then(Value::as_str)
            .unwrap_or("main")
            .to_string())
    }

    async fn try_merge_upstream(
        &self,
        client: &ghc_api::client::Client,
        dest_repo: &Repo,
        branch: &str,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
    ) -> Result<Option<()>> {
        let merge_path = format!(
            "repos/{}/{}/merge-upstream",
            dest_repo.owner(),
            dest_repo.name()
        );
        let merge_body = serde_json::json!({ "branch": branch });

        match client
            .rest::<Value>(reqwest::Method::POST, &merge_path, Some(&merge_body))
            .await
        {
            Ok(resp) => {
                let base_branch = resp
                    .get("base_branch")
                    .and_then(Value::as_str)
                    .unwrap_or(branch);
                let branch_name = base_branch.rsplit(':').next().unwrap_or(branch);

                if ios.is_stdout_tty() {
                    ios_println!(
                        ios,
                        "{} Synced the \"{}:{}\" branch from \"{}\"",
                        cs.success_icon(),
                        dest_repo.owner(),
                        branch_name,
                        base_branch
                    );
                }
                Ok(Some(()))
            }
            Err(ghc_api::errors::ApiError::Http { status, .. })
                if status == 409 || status == 422 =>
            {
                Ok(None)
            }
            Err(e) => Err(e).context("failed to sync via merge-upstream"),
        }
    }

    async fn sync_via_git_refs(
        &self,
        client: &ghc_api::client::Client,
        dest_repo: &Repo,
        branch: &str,
        ios: &ghc_core::iostreams::IOStreams,
        cs: &ghc_core::iostreams::ColorScheme,
    ) -> Result<()> {
        let src_repo = if let Some(s) = &self.source {
            Repo::from_full_name(s).context("invalid source repository")?
        } else {
            resolve_parent_repo(client, dest_repo).await?
        };

        if dest_repo.host() != src_repo.host() {
            anyhow::bail!("can't sync repositories from different hosts");
        }

        let sha = get_branch_sha(client, &src_repo, branch).await?;

        let dest_ref_path = format!(
            "repos/{}/{}/git/refs/heads/{}",
            dest_repo.owner(),
            dest_repo.name(),
            branch
        );
        let update_body = serde_json::json!({
            "sha": sha,
            "force": self.force,
        });

        match client
            .rest::<Value>(reqwest::Method::PATCH, &dest_ref_path, Some(&update_body))
            .await
        {
            Ok(_) => {}
            Err(ghc_api::errors::ApiError::Http { message, .. }) => {
                if message.contains("Update is not a fast forward") {
                    anyhow::bail!(
                        "can't sync because there are diverging changes; use `--force` to overwrite the destination branch"
                    );
                }
                if message.contains("Reference does not exist") {
                    anyhow::bail!(
                        "{branch} branch does not exist on {} repository",
                        dest_repo.full_name()
                    );
                }
                anyhow::bail!("failed to sync: {message}");
            }
            Err(e) => return Err(e).context("failed to sync repository"),
        }

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Synced the \"{}:{}\" branch from \"{}:{}\"",
                cs.success_icon(),
                dest_repo.owner(),
                branch,
                src_repo.owner(),
                branch
            );
        }

        Ok(())
    }
}

async fn find_remote_name(
    git_client: &ghc_git::client::GitClient,
    src_repo: &Repo,
) -> Result<String> {
    let remotes = git_client
        .remotes()
        .await
        .context("failed to list remotes")?;
    let remote = remotes
        .iter()
        .find(|r| {
            r.repo.as_ref().is_some_and(|repo| {
                repo.owner() == src_repo.owner() && repo.name() == src_repo.name()
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "can't find corresponding remote for {}",
                src_repo.full_name()
            )
        })?;
    Ok(remote.name.clone())
}

async fn resolve_parent_repo(client: &ghc_api::client::Client, dest_repo: &Repo) -> Result<Repo> {
    let mut variables = HashMap::new();
    variables.insert("owner".into(), Value::String(dest_repo.owner().to_string()));
    variables.insert("name".into(), Value::String(dest_repo.name().to_string()));

    let data: Value = client
        .graphql(PARENT_REPO_QUERY, &variables)
        .await
        .context("failed to fetch parent repository")?;

    let parent = data.pointer("/repository/parent").ok_or_else(|| {
        anyhow::anyhow!(
            "can't determine source repository for {} because repository is not fork",
            dest_repo.full_name()
        )
    })?;

    let owner = parent
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = parent.get("name").and_then(Value::as_str).unwrap_or("");

    if owner.is_empty() || name.is_empty() {
        anyhow::bail!(
            "can't determine source repository for {} because repository is not fork",
            dest_repo.full_name()
        );
    }

    Ok(Repo::new(owner, name))
}

async fn get_branch_sha(
    client: &ghc_api::client::Client,
    repo: &Repo,
    branch: &str,
) -> Result<String> {
    let ref_path = format!(
        "repos/{}/{}/git/refs/heads/{}",
        repo.owner(),
        repo.name(),
        branch
    );
    let ref_data: Value = client
        .rest(reqwest::Method::GET, &ref_path, None)
        .await
        .context("failed to get latest commit from source")?;

    ref_data
        .pointer("/object/sha")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("could not determine source commit SHA"))
}

const DEFAULT_BRANCH_QUERY: &str = r"
query RepoDefaultBranch($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    defaultBranchRef {
      name
    }
  }
}
";

const PARENT_REPO_QUERY: &str = r"
query RepoParent($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    parent {
      name
      owner { login }
    }
  }
}
";

/// Extension trait for `GitClient` to add sync-specific operations.
trait GitClientSyncExt {
    async fn run_merge_base_check(&self, branch: &str, target: &str) -> bool;
    async fn reset_hard(&self, target: &str) -> Result<(), ghc_git::errors::GitError>;
    async fn update_ref(&self, branch: &str, target: &str)
    -> Result<(), ghc_git::errors::GitError>;
    async fn create_branch_from(
        &self,
        branch: &str,
        target: &str,
        upstream: &str,
    ) -> Result<(), ghc_git::errors::GitError>;
}

impl GitClientSyncExt for ghc_git::client::GitClient {
    async fn run_merge_base_check(&self, branch: &str, target: &str) -> bool {
        let args = ["merge-base", "--is-ancestor", branch, target];
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(args);
        if let Some(dir) = self.repo_dir() {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        matches!(cmd.status().await, Ok(s) if s.success())
    }

    async fn reset_hard(&self, target: &str) -> Result<(), ghc_git::errors::GitError> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["reset", "--hard", target]);
        if let Some(dir) = self.repo_dir() {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());
        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ghc_git::errors::GitError::CommandFailed {
                command: "reset".to_string(),
                message: stderr.trim().to_string(),
                exit_code: output.status.code(),
            });
        }
        Ok(())
    }

    async fn update_ref(
        &self,
        branch: &str,
        target: &str,
    ) -> Result<(), ghc_git::errors::GitError> {
        let ref_name = format!("refs/heads/{branch}");
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["update-ref", &ref_name, target]);
        if let Some(dir) = self.repo_dir() {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());
        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ghc_git::errors::GitError::CommandFailed {
                command: "update-ref".to_string(),
                message: stderr.trim().to_string(),
                exit_code: output.status.code(),
            });
        }
        Ok(())
    }

    async fn create_branch_from(
        &self,
        branch: &str,
        target: &str,
        upstream: &str,
    ) -> Result<(), ghc_git::errors::GitError> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["branch", branch, target]);
        if let Some(dir) = self.repo_dir() {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());
        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ghc_git::errors::GitError::CommandFailed {
                command: "branch".to_string(),
                message: stderr.trim().to_string(),
                exit_code: output.status.code(),
            });
        }

        // Set upstream
        let mut cmd2 = tokio::process::Command::new("git");
        cmd2.args(["branch", "--set-upstream-to", upstream, branch]);
        if let Some(dir) = self.repo_dir() {
            cmd2.current_dir(dir);
        }
        cmd2.stdout(std::process::Stdio::null());
        cmd2.stderr(std::process::Stdio::piped());
        let output2 = cmd2.output().await?;
        if !output2.status.success() {
            let stderr = String::from_utf8_lossy(&output2.stderr);
            return Err(ghc_git::errors::GitError::CommandFailed {
                command: "branch".to_string(),
                message: stderr.trim().to_string(),
                exit_code: output2.status.code(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql, mock_rest_post};

    #[tokio::test]
    async fn test_should_sync_remote_via_merge_upstream() {
        let h = TestHarness::new().await;

        mock_graphql(
            &h.server,
            "RepoDefaultBranch",
            serde_json::json!({
                "data": {
                    "repository": {
                        "defaultBranchRef": { "name": "main" }
                    }
                }
            }),
        )
        .await;

        mock_rest_post(
            &h.server,
            "/repos/fork-owner/repo/merge-upstream",
            200,
            serde_json::json!({
                "message": "Successfully fetched and fast-forwarded from upstream",
                "merge_type": "fast-forward",
                "base_branch": "upstream-owner:main",
            }),
        )
        .await;

        let args = SyncArgs {
            destination: Some("fork-owner/repo".into()),
            source: None,
            branch: None,
            force: false,
        };
        // Succeeds without error (TTY output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }
}
