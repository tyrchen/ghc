//! Copilot command (`ghc copilot`).
//!
//! Interact with GitHub Copilot from the CLI.

use anyhow::Result;
use clap::Subcommand;
use ghc_core::ios_println;
use serde_json::Value;

/// Interact with GitHub Copilot.
#[derive(Debug, Subcommand)]
pub enum CopilotCommand {
    /// Explain code or a command.
    Explain(ExplainArgs),
    /// Suggest a shell command.
    Suggest(SuggestArgs),
}

impl CopilotCommand {
    /// Run the copilot subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        match self {
            Self::Explain(args) => args.run(factory).await,
            Self::Suggest(args) => args.run(factory).await,
        }
    }
}

/// Explain code or a command.
#[derive(Debug, clap::Args)]
pub struct ExplainArgs {
    /// The text to explain.
    #[arg(value_name = "TEXT")]
    text: Vec<String>,
}

impl ExplainArgs {
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;

        let client = factory.api_client("github.com")?;
        let prompt = self.text.join(" ");

        if prompt.is_empty() {
            anyhow::bail!("text to explain is required");
        }

        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": format!("Explain: {prompt}")
                }
            ]
        });

        let result: serde_json::Value = client
            .rest(
                reqwest::Method::POST,
                "copilot/chat/completions",
                Some(&body),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Copilot API request failed: {e}"))?;

        let content = result
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("No response from Copilot");

        ios_println!(ios, "{content}");

        Ok(())
    }
}

/// Suggest a shell command.
#[derive(Debug, clap::Args)]
pub struct SuggestArgs {
    /// Description of what you want to do.
    #[arg(value_name = "TEXT")]
    text: Vec<String>,

    /// Target shell (bash, zsh, fish, powershell).
    #[arg(short, long, default_value = "bash")]
    shell: String,
}

impl SuggestArgs {
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let client = factory.api_client("github.com")?;
        let prompt = self.text.join(" ");

        if prompt.is_empty() {
            anyhow::bail!("description of what you want to do is required");
        }

        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": format!("Suggest a {} shell command to: {}", self.shell, prompt)
                }
            ]
        });

        let result: serde_json::Value = client
            .rest(
                reqwest::Method::POST,
                "copilot/chat/completions",
                Some(&body),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Copilot API request failed: {e}"))?;

        let content = result
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("No suggestion from Copilot");

        ios_println!(ios, "{content}");

        Ok(())
    }
}
