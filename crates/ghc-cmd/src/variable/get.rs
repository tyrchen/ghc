//! `ghc variable get` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// Get a variable value.
#[derive(Debug, Args)]
pub struct GetArgs {
    /// The variable name.
    #[arg(value_name = "VARIABLE_NAME")]
    name: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Get an organization variable.
    #[arg(short, long)]
    org: Option<String>,

    /// Get an environment variable.
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

impl GetArgs {
    /// Run the variable get command.
    ///
    /// # Errors
    ///
    /// Returns an error if the variable cannot be retrieved.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;

        let client = factory.api_client("github.com")?;

        let path = if let Some(ref org) = self.org {
            format!("orgs/{org}/actions/variables/{}", self.name)
        } else if let Some(ref env) = self.env {
            let repo = self
                .repo
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("repository required for environment variables"))?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/environments/{env}/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        } else {
            let repo = self.repo.as_deref().ok_or_else(|| {
                anyhow::anyhow!("repository argument required (use -R OWNER/REPO)")
            })?;
            let repo = Repo::from_full_name(repo).context("invalid repository format")?;
            format!(
                "repos/{}/{}/actions/variables/{}",
                repo.owner(),
                repo.name(),
                self.name,
            )
        };

        let variable: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to get variable")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &variable,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let value = variable.get("value").and_then(Value::as_str).unwrap_or("");
        ios_println!(ios, "{value}");

        Ok(())
    }
}
