//! `ghc variable list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List variables.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// List organization variables.
    #[arg(short, long)]
    org: Option<String>,

    /// List environment variables.
    #[arg(short, long)]
    env: Option<String>,

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
    /// Run the variable list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the variables cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/variables")
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment variables"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/variables",
                repo.owner(),
                repo.name(),
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!("repos/{}/{}/actions/variables", repo.owner(), repo.name(),)
        };

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list variables")?;

        // Extract inner array from wrapper object
        let items = result
            .get("variables")
            .cloned()
            .unwrap_or(Value::Array(vec![]));

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let mut items_owned = items.clone();
            normalize_variable_fields(&mut items_owned);
            let output = ghc_core::json::format_json_output(
                &items_owned,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let variables = items
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("unexpected response format"))?;

        if variables.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No variables found");
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for var in variables {
            let name = var.get("name").and_then(Value::as_str).unwrap_or("");
            let value = var.get("value").and_then(Value::as_str).unwrap_or("");
            let updated_at = var.get("updated_at").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                cs.bold(name),
                value.to_string(),
                updated_at.to_string(),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

/// Normalize variable fields to match gh CLI conventions.
///
/// Ensures `visibility` field is present (empty string for repo-level variables),
/// maps `updated_at` -> `updatedAt`, `created_at` -> `createdAt`.
fn normalize_variable_fields(value: &mut Value) {
    if let Some(arr) = value.as_array_mut() {
        for var in arr {
            if let Some(obj) = var.as_object_mut() {
                // Ensure visibility is present
                if !obj.contains_key("visibility") {
                    obj.insert("visibility".to_string(), Value::String(String::new()));
                }
                // Map snake_case -> camelCase
                if let Some(val) = obj.get("updated_at").cloned() {
                    obj.insert("updatedAt".to_string(), val);
                }
                if let Some(val) = obj.get("created_at").cloned() {
                    obj.insert("createdAt".to_string(), val);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_list_repo_variables() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/actions/variables",
            serde_json::json!({
                "total_count": 2,
                "variables": [
                    {"name": "NODE_ENV", "value": "production", "updated_at": "2024-01-15T10:00:00Z"},
                    {"name": "DEPLOY_TARGET", "value": "staging", "updated_at": "2024-01-14T10:00:00Z"}
                ]
            }),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            org: None,
            env: None,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("NODE_ENV"), "should contain variable name");
        assert!(
            stdout.contains("production"),
            "should contain variable value"
        );
    }
}
