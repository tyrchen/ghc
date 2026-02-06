//! `ghc ruleset view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;
use ghc_core::repo::Repo;

/// View a ruleset.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Ruleset ID.
    #[arg(value_name = "RULESET_ID")]
    id: u64,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Open the ruleset in the browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the ruleset view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the ruleset cannot be viewed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;

        if self.web {
            let url = format!(
                "https://{}/{}/{}/rules/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.id,
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;

        let path = format!(
            "repos/{}/{}/rulesets/{}",
            repo.owner(),
            repo.name(),
            self.id,
        );

        let ruleset: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch ruleset")?;

        let ios = &factory.io;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&ruleset)?);
            return Ok(());
        }

        let cs = ios.color_scheme();

        let name = ruleset.get("name").and_then(Value::as_str).unwrap_or("");
        let enforcement = ruleset
            .get("enforcement")
            .and_then(Value::as_str)
            .unwrap_or("");
        let source_type = ruleset
            .get("source_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let target = ruleset.get("target").and_then(Value::as_str).unwrap_or("");

        ios_println!(ios, "{}", cs.bold(name));
        ios_println!(ios, "ID: {}", self.id);
        ios_println!(ios, "Target: {target}");
        ios_println!(ios, "Source: {source_type}");
        ios_println!(ios, "Enforcement: {enforcement}");

        // Show rules
        if let Some(rules) = ruleset.get("rules").and_then(Value::as_array) {
            ios_println!(ios, "\nRules ({} total):", rules.len());
            for rule in rules {
                let rule_type = rule
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                ios_println!(ios, "  - {rule_type}");
            }
        }

        // Show conditions
        if let Some(conditions) = ruleset.get("conditions") {
            ios_println!(ios, "\nConditions:");
            let conditions_str =
                serde_json::to_string_pretty(conditions).unwrap_or_else(|_| "{}".to_string());
            ios_println!(ios, "{conditions_str}");
        }

        Ok(())
    }
}
