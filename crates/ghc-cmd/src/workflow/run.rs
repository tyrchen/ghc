//! `ghc workflow run` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;
use ghc_core::repo::Repo;

/// Trigger a workflow run.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Workflow ID or filename.
    #[arg(value_name = "WORKFLOW")]
    workflow: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Git ref (branch or tag) to run the workflow on.
    #[arg(short, long, default_value = "main")]
    r#ref: String,

    /// Input parameters as KEY=VALUE pairs.
    #[arg(short = 'f', long = "field", value_name = "KEY=VALUE")]
    fields: Vec<String>,

    /// Read input parameters as JSON from stdin.
    #[arg(long)]
    json_input: bool,
}

impl RunArgs {
    /// Run the workflow run command.
    ///
    /// # Errors
    ///
    /// Returns an error if the workflow cannot be triggered.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let mut inputs: HashMap<String, String> = HashMap::new();

        if self.json_input {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .context("failed to read from stdin")?;
            let json_inputs: HashMap<String, String> =
                serde_json::from_str(&buf).context("failed to parse JSON input")?;
            inputs.extend(json_inputs);
        }

        for field in &self.fields {
            let (key, value) = field.split_once('=').ok_or_else(|| {
                anyhow::anyhow!("invalid field format: {field}, expected KEY=VALUE")
            })?;
            inputs.insert(key.to_string(), value.to_string());
        }

        let body = serde_json::json!({
            "ref": self.r#ref,
            "inputs": inputs,
        });

        let path = format!(
            "repos/{}/{}/actions/workflows/{}/dispatches",
            repo.owner(),
            repo.name(),
            self.workflow,
        );

        client
            .rest_text(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to trigger workflow")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(
            ios,
            "{} Triggered workflow {} on ref {}",
            cs.success_icon(),
            cs.bold(&self.workflow),
            cs.bold(&self.r#ref),
        );

        Ok(())
    }
}
