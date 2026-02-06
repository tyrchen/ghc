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

    /// List secrets for your user (Codespaces).
    #[arg(short, long)]
    user: bool,

    /// List secrets for a specific application (actions, codespaces, or dependabot).
    #[arg(short, long, value_parser = ["actions", "codespaces", "dependabot"])]
    app: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ListArgs {
    /// Run the secret list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the secrets cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let entity_count =
            u8::from(self.org.is_some()) + u8::from(self.env.is_some()) + u8::from(self.user);
        if entity_count > 1 {
            anyhow::bail!("specify only one of `--org`, `--env`, or `--user`");
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let app = if let Some(ref a) = self.app {
            a.as_str()
        } else if self.user {
            "codespaces"
        } else {
            "actions"
        };

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/{app}/secrets")
        } else if self.user {
            "user/codespaces/secrets".to_string()
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
            format!("repos/{}/{}/{app}/secrets", repo.owner(), repo.name(),)
        };

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list secrets")?;

        // Extract inner array from wrapper object
        let items = result
            .get("secrets")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &items,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let secrets = items
            .as_array()
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
            user: false,
            app: None,
            json: vec![],
            jq: None,
            template: None,
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
            user: false,
            app: None,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("ORG_SECRET"), "should contain org secret");
    }
}
