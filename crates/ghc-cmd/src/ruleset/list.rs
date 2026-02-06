//! `ghc ruleset list` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List rulesets for a repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Include rulesets from parent organizations.
    #[arg(long)]
    parents: bool,

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
    /// Run the ruleset list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the rulesets cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;

        let mut path = format!("repos/{}/{}/rulesets", repo.owner(), repo.name(),);
        if self.parents {
            path.push_str("?includes_parents=true");
        }

        let rulesets: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list rulesets")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let arr = Value::Array(rulesets.clone());
            let output = ghc_core::json::format_json_output(
                &arr,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        if rulesets.is_empty() {
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "No rulesets found in {}", repo.full_name());
            }
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for ruleset in &rulesets {
            let id = ruleset.get("id").and_then(Value::as_u64).unwrap_or(0);
            let name = ruleset.get("name").and_then(Value::as_str).unwrap_or("");
            let source_type = ruleset
                .get("source_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let enforcement = ruleset
                .get("enforcement")
                .and_then(Value::as_str)
                .unwrap_or("");

            let enforcement_display = match enforcement {
                "active" => cs.success("active"),
                "evaluate" => cs.warning("evaluate"),
                "disabled" => cs.gray("disabled"),
                _ => enforcement.to_string(),
            };

            tp.add_row(vec![
                format!("{id}"),
                cs.bold(name),
                source_type.to_string(),
                enforcement_display,
            ]);
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
    async fn test_should_list_rulesets() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/rulesets",
            serde_json::json!([
                {
                    "id": 1,
                    "name": "Branch Protection",
                    "source_type": "Repository",
                    "enforcement": "active"
                },
                {
                    "id": 2,
                    "name": "Tag Protection",
                    "source_type": "Repository",
                    "enforcement": "evaluate"
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: Some("owner/repo".to_string()),
            parents: false,
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(
            stdout.contains("Branch Protection"),
            "should contain ruleset name"
        );
        assert!(
            stdout.contains("Tag Protection"),
            "should contain second ruleset"
        );
    }
}
