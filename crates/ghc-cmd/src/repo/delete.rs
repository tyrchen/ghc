//! `ghc repo delete` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Delete a GitHub repository.
///
/// With no argument, deletes the current repository. Otherwise, deletes the
/// specified repository.
///
/// Deletion requires authorization with the `delete_repo` scope.
/// To authorize, run `ghc auth refresh -s delete_repo`.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Repository to delete (OWNER/REPO or just REPO for current user).
    #[arg(value_name = "REPOSITORY")]
    repo: Option<String>,

    /// Confirm deletion without prompting.
    #[arg(long)]
    yes: bool,
}

impl DeleteArgs {
    /// Run the repo delete command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let client = factory.api_client("github.com")?;

        let repo_arg = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (e.g. OWNER/REPO)"))?;

        // If the arg doesn't contain '/', prepend the current user
        let full_name = if repo_arg.contains('/') {
            repo_arg.to_string()
        } else {
            let current_user = client
                .current_login()
                .await
                .context("failed to get current user")?;
            format!("{current_user}/{repo_arg}")
        };

        let repo = Repo::from_full_name(&full_name).context("argument error")?;
        let display_name = repo.full_name();

        // Require confirmation
        if !self.yes {
            if !ios.can_prompt() {
                anyhow::bail!("--yes required when not running interactively");
            }
            let answer = factory
                .prompter()
                .input(&format!("Type {display_name} to confirm deletion:"), "")?;
            if answer != display_name {
                anyhow::bail!("confirmation did not match repository name");
            }
        }

        let delete_path = format!("repos/{}/{}", repo.owner(), repo.name());
        let resp = client
            .rest_text(reqwest::Method::DELETE, &delete_path, None)
            .await;

        match resp {
            Ok(_) => {}
            Err(ghc_api::errors::ApiError::Http {
                status, message, ..
            }) if status == 301 || status == 307 || status == 308 => {
                ios_eprintln!(
                    ios,
                    "{} Failed to delete repository: {} has changed name or transferred ownership",
                    cs.error_icon(),
                    display_name
                );
                anyhow::bail!("{message}");
            }
            Err(e) => {
                return Err(e).context("failed to delete repository");
            }
        }

        if ios.is_stdout_tty() {
            ios_println!(
                ios,
                "{} Deleted repository {}",
                cs.success_icon(),
                display_name
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_delete};

    #[tokio::test]
    async fn test_should_delete_repository_with_yes() {
        let h = TestHarness::new().await;
        mock_rest_delete(&h.server, "/repos/owner/repo", 204).await;

        let args = DeleteArgs {
            repo: Some("owner/repo".into()),
            yes: true,
        };
        // Succeeds without error (TTY output not checked since test IO is non-TTY)
        args.run(&h.factory).await.unwrap();
    }

    #[tokio::test]
    async fn test_should_fail_without_repo_argument() {
        let h = TestHarness::new().await;

        let args = DeleteArgs {
            repo: None,
            yes: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("required"));
    }

    #[tokio::test]
    async fn test_should_require_yes_when_not_interactive() {
        let h = TestHarness::new().await;

        let args = DeleteArgs {
            repo: Some("owner/repo".into()),
            yes: false,
        };
        // Test factory doesn't support prompts (can_prompt returns false)
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--yes required"));
    }
}
