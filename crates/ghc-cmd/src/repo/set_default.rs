//! `ghc repo set-default` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

use ghc_git::remote::Remote;

/// Configure default repository for this directory.
///
/// This command sets the default remote repository to use when querying the
/// GitHub API for the locally cloned repository.
///
/// ghc uses the default repository for things like:
/// - viewing and creating pull requests
/// - viewing and creating issues
/// - viewing and creating releases
/// - working with GitHub Actions
#[derive(Debug, Args)]
pub struct SetDefaultArgs {
    /// Repository to set as default (OWNER/REPO or remote name).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// View the current default repository.
    #[arg(short, long)]
    view: bool,

    /// Unset the current default repository.
    #[arg(short, long)]
    unset: bool,
}

impl SetDefaultArgs {
    /// Run the repo set-default command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let git_client = factory.git_client()?;

        if !git_client.is_repo().await? {
            anyhow::bail!("must be run from inside a git repository");
        }

        let remotes = git_client
            .remotes()
            .await
            .context("failed to list remotes")?;

        let current_default = remotes.iter().find(|r| !r.resolved.is_empty());

        if self.view {
            self.handle_view(factory, current_default);
            return Ok(());
        }

        if self.unset {
            return self
                .handle_unset(factory, git_client, current_default)
                .await;
        }

        self.handle_set(factory, git_client, &remotes, current_default)
            .await
    }

    #[allow(clippy::unused_self)]
    fn handle_view(&self, factory: &crate::factory::Factory, current_default: Option<&Remote>) {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if let Some(remote) = current_default {
            let resolved = &remote.resolved;
            let display = if resolved.is_empty() || resolved == "base" {
                remote_full_name(remote)
            } else if let Ok(repo) = Repo::from_full_name(resolved) {
                repo.full_name()
            } else {
                remote_full_name(remote)
            };
            ios_println!(ios, "{display}");
        } else {
            ios_println!(
                ios,
                "{} No default remote repository has been set. \
                 To learn more about the default repository, run: ghc repo set-default --help",
                cs.error_icon()
            );
        }
    }

    async fn handle_unset(
        &self,
        factory: &crate::factory::Factory,
        git_client: &ghc_git::client::GitClient,
        current_default: Option<&Remote>,
    ) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if let Some(remote) = current_default {
            git_client
                .unset_remote_resolution(&remote.name)
                .await
                .context("failed to unset remote resolution")?;

            let repo_name = remote_full_name(remote);
            if ios.is_stdout_tty() {
                ios_println!(
                    ios,
                    "{} Unset {} as default repository",
                    cs.success_icon(),
                    repo_name
                );
            }
        } else if ios.is_stdout_tty() {
            ios_println!(ios, "no default repository has been set");
        }
        Ok(())
    }

    async fn handle_set(
        &self,
        factory: &crate::factory::Factory,
        git_client: &ghc_git::client::GitClient,
        remotes: &[Remote],
        current_default: Option<&Remote>,
    ) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let target_repo = if let Some(r) = &self.repo {
            self.resolve_repo_arg(r, remotes)?
        } else {
            self.prompt_for_repo(factory, remotes, current_default)?
        };

        let target_remote = remotes
            .iter()
            .find(|r| remote_matches_repo(r, target_repo.owner(), target_repo.name()))
            .or_else(|| remotes.first())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} does not correspond to any git remotes",
                    target_repo.full_name()
                )
            })?;

        if let Some(current) = current_default {
            git_client.unset_remote_resolution(&current.name).await.ok();
        }

        let resolution =
            if remote_matches_repo(target_remote, target_repo.owner(), target_repo.name()) {
                "base".to_string()
            } else {
                target_repo.full_name()
            };

        git_client
            .set_remote_resolution(&target_remote.name, &resolution)
            .await
            .context("failed to set remote resolution")?;

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Set {} as the default repository for the current directory",
                cs.success_icon(),
                target_repo.full_name()
            );
        }

        Ok(())
    }

    #[allow(clippy::unused_self)]
    fn resolve_repo_arg(&self, arg: &str, remotes: &[Remote]) -> Result<Repo> {
        if let Ok(repo) = Repo::from_full_name(arg) {
            return Ok(repo);
        }
        let remote = remotes
            .iter()
            .find(|rem| rem.name == *arg)
            .ok_or_else(|| anyhow::anyhow!("given arg is not a valid repo or git remote: {arg}"))?;
        let (owner, name) = remote_owner_name(remote);
        Ok(Repo::new(owner, name))
    }

    #[allow(clippy::unused_self)]
    fn prompt_for_repo(
        &self,
        factory: &crate::factory::Factory,
        remotes: &[Remote],
        current_default: Option<&Remote>,
    ) -> Result<Repo> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if !ios.can_prompt() {
            anyhow::bail!("repository required when not running interactively");
        }

        let mut repo_names: Vec<String> = Vec::new();
        for remote in remotes {
            if let Some(ref repo) = remote.repo {
                let full = repo.full_name();
                if !repo_names.contains(&full) {
                    repo_names.push(full);
                }
            }
        }

        if repo_names.is_empty() {
            anyhow::bail!("none of the git remotes correspond to a valid remote repository");
        }

        if repo_names.len() == 1 {
            let repo_name = &repo_names[0];
            ios_println!(
                ios,
                "Found only one known remote repo, {}.",
                cs.bold(repo_name)
            );
            return Repo::from_full_name(repo_name).map_err(Into::into);
        }

        let current = current_default.map(remote_full_name);
        let default_idx = current
            .as_ref()
            .and_then(|c| repo_names.iter().position(|n| n == c));

        let selected = factory.prompter().select(
            "Which repository should be the default?",
            default_idx,
            &repo_names
                .iter()
                .map(String::as_str)
                .map(String::from)
                .collect::<Vec<_>>(),
        )?;
        Repo::from_full_name(&repo_names[selected]).map_err(Into::into)
    }
}

/// Get the full name (owner/repo) from a Remote.
fn remote_full_name(remote: &Remote) -> String {
    if let Some(ref repo) = remote.repo {
        repo.full_name()
    } else {
        String::new()
    }
}

/// Get owner and repo name from a Remote.
fn remote_owner_name(remote: &Remote) -> (String, String) {
    if let Some(ref repo) = remote.repo {
        (repo.owner().to_string(), repo.name().to_string())
    } else {
        (String::new(), String::new())
    }
}

/// Check if a Remote matches a specific owner/repo.
fn remote_matches_repo(remote: &Remote, owner: &str, name: &str) -> bool {
    remote
        .repo
        .as_ref()
        .is_some_and(|r| r.owner() == owner && r.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_parse_repo_from_full_name() {
        let repo = Repo::from_full_name("owner/repo").unwrap();
        assert_eq!(repo.owner(), "owner");
        assert_eq!(repo.name(), "repo");
    }
}
