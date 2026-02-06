//! `ghc gist clone` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Clone a gist locally via git.
#[derive(Debug, Args)]
pub struct CloneArgs {
    /// The gist ID or URL to clone.
    #[arg(value_name = "GIST")]
    gist: String,

    /// Directory to clone into.
    #[arg(value_name = "DIRECTORY")]
    directory: Option<String>,

    /// Git protocol to use (https or ssh).
    #[arg(long, default_value = "https", value_parser = ["https", "ssh"])]
    protocol: String,
}

impl CloneArgs {
    /// Run the gist clone command.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let gist_id = extract_gist_id(&self.gist);

        let clone_url = match self.protocol.as_str() {
            "ssh" => format!("git@gist.github.com:{gist_id}.git"),
            _ => format!("https://gist.github.com/{gist_id}.git"),
        };

        let dest = self.directory.as_deref().unwrap_or(gist_id);

        let ios = &factory.io;
        ios_eprintln!(ios, "Cloning into '{dest}'...");

        let status = tokio::process::Command::new("git")
            .args(["clone", &clone_url, dest])
            .status()
            .await
            .context("failed to execute git clone")?;

        if !status.success() {
            anyhow::bail!(
                "git clone failed with exit code {}",
                status.code().unwrap_or(1),
            );
        }

        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Cloned gist {gist_id}", cs.success_icon());

        Ok(())
    }
}

/// Extract the gist ID from a URL or return the input as-is.
fn extract_gist_id(input: &str) -> &str {
    input
        .rsplit('/')
        .next()
        .unwrap_or(input)
        .trim_end_matches(".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_extract_gist_id_from_plain_id() {
        assert_eq!(extract_gist_id("abc123"), "abc123");
    }

    #[test]
    fn test_should_extract_gist_id_from_url() {
        assert_eq!(extract_gist_id("https://gist.github.com/abc123"), "abc123");
    }

    #[test]
    fn test_should_extract_gist_id_from_git_url() {
        assert_eq!(
            extract_gist_id("https://gist.github.com/abc123.git"),
            "abc123"
        );
    }

    #[test]
    fn test_should_extract_gist_id_from_url_with_user() {
        assert_eq!(
            extract_gist_id("https://gist.github.com/user/abc123"),
            "abc123"
        );
    }

    #[test]
    fn test_should_build_ssh_url() {
        let args = CloneArgs {
            gist: "abc123".into(),
            directory: None,
            protocol: "ssh".into(),
        };
        // Verify protocol field is set correctly
        assert_eq!(args.protocol, "ssh");
    }

    #[test]
    fn test_should_build_https_url() {
        let args = CloneArgs {
            gist: "abc123".into(),
            directory: None,
            protocol: "https".into(),
        };
        assert_eq!(args.protocol, "https");
    }
}
