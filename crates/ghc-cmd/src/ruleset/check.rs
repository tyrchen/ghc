//! `ghc ruleset check` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Check rules that apply to a branch.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Branch name to check.
    #[arg(value_name = "BRANCH", default_value = "main")]
    branch: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl CheckArgs {
    /// Run the ruleset check command.
    ///
    /// # Errors
    ///
    /// Returns an error if the rules cannot be checked.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;

        let path = format!(
            "repos/{}/{}/rules/branches/{}",
            repo.owner(),
            repo.name(),
            self.branch,
        );

        let rules: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to check branch rules")?;

        let ios = &factory.io;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&rules)?);
            return Ok(());
        }

        if rules.is_empty() {
            ios_eprintln!(
                ios,
                "No rules apply to branch {} in {}",
                self.branch,
                repo.full_name(),
            );
            return Ok(());
        }

        let cs = ios.color_scheme();
        ios_println!(
            ios,
            "Rules for branch {} in {}:",
            cs.bold(&self.branch),
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios);

        for rule in &rules {
            let rule_type = rule
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let ruleset_source = rule
                .get("ruleset_source_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let ruleset_id = rule.get("ruleset_id").and_then(Value::as_u64).unwrap_or(0);

            ios_println!(
                ios,
                "  - {} (ruleset #{ruleset_id}, source: {ruleset_source})",
                cs.bold(rule_type),
            );

            if let Some(parameters) = rule.get("parameters")
                && let Some(params_obj) = parameters.as_object()
            {
                for (key, value) in params_obj {
                    ios_println!(ios, "      {key}: {value}");
                }
            }
        }

        Ok(())
    }
}
