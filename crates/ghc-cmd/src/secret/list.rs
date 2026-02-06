//! `ghc secret list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List secrets.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// List organization secrets.
    #[arg(short, long)]
    org: Option<String>,

    /// List environment secrets.
    #[arg(short, long)]
    env: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the secret list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secrets cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/secrets")
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment secrets"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/secrets",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!("repos/{}/{}/actions/secrets", repo.owner(), repo.name(),)
        };

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list secrets")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        let secrets = result
            .get("secrets")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if secrets.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No secrets found");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for secret in secrets {
            let name = secret.get("name").and_then(Value::as_str).unwrap_or("");
            let updated_at = secret
                .get("updated_at")
                .and_then(Value::as_str)
                .unwrap_or("");

            tp.add_row(vec![cs.bold(name), updated_at.to_string()]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_repo_secrets() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/secrets",
            serde_json::json!({
                "total_count": 2,
                "secrets": [
                    {"name": "DEPLOY_KEY", "updated_at": "2024-01-15T10:00:00Z"},
                    {"name": "NPM_TOKEN", "updated_at": "2024-01-14T10:00:00Z"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            org: None,
            env: None,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("DEPLOY_KEY"), "should contain secret name");
        assert!(stdout.contains("NPM_TOKEN"), "should contain second secret");
    }

    #[tokio::test]
    async fn test_should_list_org_secrets() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/orgs/myorg/actions/secrets",
            serde_json::json!({
                "total_count": 1,
                "secrets": [
                    {"name": "ORG_SECRET", "updated_at": "2024-01-15T10:00:00Z"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: None,
            org: Some("myorg".to_string()),
            env: None,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("ORG_SECRET"), "should contain org secret");
    }
}
