//! `ghc codespace view` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::ios_println;

/// View details about a codespace.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// Name of the codespace to view.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    /// Run the codespace view command.
    ///
    /// # Errors
    ///
    /// Returns an error if the codespace cannot be viewed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!("user/codespaces/{codespace_name}");
        let codespace: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to fetch codespace")?;

        // JSON output
        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&codespace)?);
            return Ok(());
        }

        let name = codespace.get("name").and_then(Value::as_str).unwrap_or("");
        let display_name = codespace
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or(name);
        let state = codespace.get("state").and_then(Value::as_str).unwrap_or("");
        let repo = codespace
            .pointer("/repository/full_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let branch = codespace
            .pointer("/git_status/ref")
            .and_then(Value::as_str)
            .unwrap_or("");
        let machine = codespace
            .pointer("/machine/display_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let created_at = codespace
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("");
        let updated_at = codespace
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or("");

        let state_display = match state {
            "Available" => cs.success("available"),
            "Shutdown" => cs.gray("stopped"),
            "Rebuilding" | "Starting" => cs.warning(state),
            _ => state.to_string(),
        };

        ios_println!(ios, "{}", cs.bold(display_name));
        ios_println!(ios, "Name: {name}");
        ios_println!(ios, "Repository: {repo}");
        ios_println!(ios, "Branch: {branch}");
        ios_println!(ios, "State: {state_display}");
        ios_println!(ios, "Machine: {machine}");
        ios_println!(ios, "Created: {created_at}");
        ios_println!(ios, "Updated: {updated_at}");

        Ok(())
    }
}
