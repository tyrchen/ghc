//! `ghc repo clone` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;
use ghc_git::url_parser;

/// Clone a repository locally.
#[derive(Debug, Args)]
pub struct CloneArgs {
    /// Repository to clone (OWNER/REPO or URL).
    #[arg(value_name = "REPOSITORY")]
    repo: String,

    /// Directory to clone into.
    #[arg(value_name = "DIRECTORY")]
    directory: Option<String>,

    /// Additional git clone arguments.
    #[arg(last = true)]
    git_args: Vec<String>,
}

impl CloneArgs {
    /// Run the repo clone command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let git = factory.git_client()?;

        // Determine clone URL
        let clone_url = if self.repo.contains("://") || self.repo.starts_with("git@") {
            self.repo.clone()
        } else {
            let repo = Repo::from_full_name(&self.repo)
                .context("invalid repository format, expected OWNER/REPO")?;

            let protocol = factory
                .config()
                .ok()
                .and_then(|c| {
                    let cfg = c.lock().ok()?;
                    Some(cfg.git_protocol(repo.host()))
                })
                .unwrap_or_else(|| "https".to_string());

            url_parser::clone_url(&repo, &protocol)
        };

        let dest = self.directory.as_deref().unwrap_or_else(|| {
            // Extract repo name from URL for default directory
            let url = &self.repo;
            url.rsplit('/')
                .next()
                .unwrap_or(url)
                .trim_end_matches(".git")
        });

        // Build git clone command args
        let mut args = vec!["clone", &clone_url, dest];
        let git_arg_refs: Vec<&str> = self.git_args.iter().map(String::as_str).collect();
        args.extend(git_arg_refs);

        let ios = &factory.io;
        ios_eprintln!(ios, "Cloning into '{dest}'...");
        git.checkout(dest).await.ok(); // Just use the low-level command
        // Actually we need a raw git command. Let's use the client
        // For now, shell out to git clone directly
        let status = tokio::process::Command::new("git")
            .args(["clone", &clone_url, dest])
            .args(&self.git_args)
            .status()
            .await
            .context("failed to execute git clone")?;

        if !status.success() {
            anyhow::bail!(
                "git clone failed with exit code {}",
                status.code().unwrap_or(1)
            );
        }

        Ok(())
    }
}
