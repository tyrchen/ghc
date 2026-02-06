//! `ghc repo rename` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Rename a GitHub repository.
///
/// `<new-name>` is the desired repository name without the owner.
///
/// By default, the current repository is renamed. Otherwise, the repository
/// specified with `--repo` is renamed.
///
/// To transfer repository ownership to another user account or organization,
/// you must follow additional steps on github.com.
#[derive(Debug, Args)]
pub struct RenameArgs {
    /// New name for the repository (without the owner prefix).
    #[arg(value_name = "NEW_NAME")]
    new_name: Option<String>,

    /// Repository to rename (OWNER/REPO).
    #[arg(short = 'R', long = "repo")]
    repo_override: Option<String>,

    /// Skip the confirmation prompt.
    #[arg(short, long)]
    yes: bool,
}

impl RenameArgs {
    /// Run the repo rename command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let repo = if let Some(r) = &self.repo_override {
            Repo::from_full_name(r).context("invalid repository format")?
        } else {
            anyhow::bail!("repository argument required (use -R OWNER/REPO)")
        };

        let client = factory.api_client(repo.host())?;
        let new_name = self.resolve_new_name(factory, &repo)?;

        if new_name.contains('/') {
            anyhow::bail!(
                "New repository name cannot contain '/' character - to transfer a repository \
                 to a new owner, see <https://docs.github.com/en/repositories/creating-and-managing-repositories/transferring-a-repository>"
            );
        }

        self.confirm_rename(factory, &repo, &new_name)?;

        let api_path = format!("repos/{}/{}", repo.owner(), repo.name());
        let body = serde_json::json!({ "name": new_name });

        let result: Value = client
            .rest(reqwest::Method::PATCH, &api_path, Some(&body))
            .await
            .context("failed to rename repository")?;

        let new_full_name = result
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(&new_name);

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Renamed repository {}",
                cs.success_icon(),
                new_full_name
            );
        }

        if self.repo_override.is_some() {
            return Ok(());
        }

        self.update_git_remote(factory, &repo, &new_name).await?;
        Ok(())
    }

    fn resolve_new_name(&self, factory: &crate::factory::Factory, repo: &Repo) -> Result<String> {
        let ios = &factory.io;
        if let Some(n) = &self.new_name {
            Ok(n.clone())
        } else {
            if !ios.can_prompt() {
                anyhow::bail!("new name argument required when not running interactively");
            }
            factory
                .prompter()
                .input(&format!("Rename {} to:", repo.full_name()), "")
        }
    }

    fn confirm_rename(
        &self,
        factory: &crate::factory::Factory,
        repo: &Repo,
        new_name: &str,
    ) -> Result<()> {
        let ios = &factory.io;
        if self.yes || self.repo_override.is_some() {
            return Ok(());
        }
        if !ios.can_prompt() {
            anyhow::bail!("--yes required when passing a single argument");
        }
        let confirmed = factory.prompter().confirm(
            &format!("Rename {} to {}?", repo.full_name(), new_name),
            false,
        )?;
        if !confirmed {
            anyhow::bail!("rename cancelled");
        }
        Ok(())
    }

    async fn update_git_remote(
        &self,
        factory: &crate::factory::Factory,
        repo: &Repo,
        new_name: &str,
    ) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let Ok(git_client) = factory.git_client() else {
            return Ok(());
        };

        // Read protocol from config before any await points
        let new_repo_url = {
            let cfg_lock = factory.config()?;
            let cfg = cfg_lock
                .lock()
                .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
            let protocol = cfg.git_protocol(repo.host());
            match protocol.as_str() {
                "ssh" => format!("git@{}:{}/{}.git", repo.host(), repo.owner(), new_name),
                _ => format!("https://{}/{}/{}.git", repo.host(), repo.owner(), new_name),
            }
        };

        let remotes = match git_client.remotes().await {
            Ok(r) => r,
            Err(e) => {
                ios_eprintln!(
                    ios,
                    "{} Warning: unable to update remote: {}",
                    cs.warning_icon(),
                    e
                );
                return Ok(());
            }
        };

        for remote in &remotes {
            let matches = remote
                .repo
                .as_ref()
                .is_some_and(|r| r.owner() == repo.owner() && r.name() == repo.name());
            if matches {
                match git_client
                    .update_remote_url(&remote.name, &new_repo_url)
                    .await
                {
                    Ok(()) => {
                        if ios.is_stdout_tty() {
                            ios_println!(
                                ios,
                                "{} Updated the {:?} remote",
                                cs.success_icon(),
                                remote.name
                            );
                        }
                    }
                    Err(e) => {
                        ios_eprintln!(
                            ios,
                            "{} Warning: unable to update remote {:?}: {}",
                            cs.warning_icon(),
                            remote.name,
                            e
                        );
                    }
                }
                break;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_patch};

    #[tokio::test]
    async fn test_should_rename_repository() {
        let h = TestHarness::new().await;
        mock_rest_patch(
            &h.server,
            "/repos/owner/old-repo",
            200,
            serde_json::json!({
                "full_name": "owner/new-repo",
                "name": "new-repo",
            }),
        )
        .await;

        let args = RenameArgs {
            new_name: Some("new-repo".into()),
            repo_override: Some("owner/old-repo".into()),
            yes: true,
        };
        // Succeeds without error (TTY output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }

    #[tokio::test]
    async fn test_should_reject_name_with_slash() {
        let h = TestHarness::new().await;

        let args = RenameArgs {
            new_name: Some("owner/new-name".into()),
            repo_override: Some("owner/old-repo".into()),
            yes: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cannot contain '/'")
        );
    }
}
