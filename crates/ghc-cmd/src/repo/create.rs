//! `ghc repo create` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_eprintln;

/// Create a new repository.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateArgs {
    /// Repository name (OWNER/REPO or just REPO for personal).
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Description of the repository.
    #[arg(short, long)]
    description: Option<String>,

    /// Make the repository public.
    #[arg(long, group = "visibility")]
    public: bool,

    /// Make the repository private.
    #[arg(long, group = "visibility")]
    private: bool,

    /// Make the repository internal.
    #[arg(long, group = "visibility")]
    internal: bool,

    /// Clone the new repository locally.
    #[arg(long)]
    clone: bool,

    /// Initialize with a README.
    #[arg(long)]
    add_readme: bool,

    /// License template (e.g., mit, apache-2.0).
    #[arg(short, long)]
    license: Option<String>,

    /// Gitignore template (e.g., Rust, Go).
    #[arg(short, long)]
    gitignore: Option<String>,

    /// Disable issues.
    #[arg(long)]
    disable_issues: bool,

    /// Disable wiki.
    #[arg(long)]
    disable_wiki: bool,
}

impl CreateArgs {
    /// Run the repo create command.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let name = self
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository name is required"))?;

        let _visibility = if self.public {
            "public"
        } else if self.internal {
            "internal"
        } else {
            "private"
        };

        let mut body = serde_json::json!({
            "name": name,
            "private": !self.public,
            "auto_init": self.add_readme,
        });

        if let Some(ref desc) = self.description {
            body["description"] = Value::String(desc.clone());
        }
        if let Some(ref license) = self.license {
            body["license_template"] = Value::String(license.clone());
        }
        if let Some(ref gitignore) = self.gitignore {
            body["gitignore_template"] = Value::String(gitignore.clone());
        }
        if self.disable_issues {
            body["has_issues"] = Value::Bool(false);
        }
        if self.disable_wiki {
            body["has_wiki"] = Value::Bool(false);
        }

        let result: Value = client
            .rest(reqwest::Method::POST, "user/repos", Some(&body))
            .await
            .context("failed to create repository")?;

        let full_name = result
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(name);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Created repository {} on GitHub",
            cs.success_icon(),
            cs.bold(full_name)
        );
        ios_eprintln!(ios, "{html_url}");

        if self.clone {
            let clone_url = result
                .get("clone_url")
                .and_then(Value::as_str)
                .unwrap_or(html_url);

            let status = tokio::process::Command::new("git")
                .args(["clone", clone_url])
                .status()
                .await
                .context("failed to clone repository")?;

            if !status.success() {
                anyhow::bail!("git clone failed");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_repository() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/user/repos",
            201,
            serde_json::json!({
                "full_name": "testuser/new-repo",
                "html_url": "https://github.com/testuser/new-repo",
                "clone_url": "https://github.com/testuser/new-repo.git",
            }),
        )
        .await;

        let args = CreateArgs {
            name: Some("new-repo".into()),
            description: Some("My new repo".into()),
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            disable_issues: false,
            disable_wiki: false,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Created repository"));
        assert!(err.contains("testuser/new-repo"));
    }

    #[tokio::test]
    async fn test_should_fail_without_name() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: None,
            description: None,
            public: false,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name is required"));
    }
}
