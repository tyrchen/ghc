//! `ghc codespace ports` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Manage codespace port forwarding.
#[derive(Debug, Args)]
pub struct PortsArgs {
    /// Name of the codespace.
    #[arg(short, long)]
    codespace: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl PortsArgs {
    /// Run the codespace ports command.
    ///
    /// # Errors
    ///
    /// Returns an error if the ports cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let codespace_name = self
            .codespace
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("codespace name required (use -c NAME)"))?;

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

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

        let cs = ios.color_scheme();
        ios_println!(ios, "Ports for codespace {}", cs.bold(codespace_name),);

        // Port info is returned when the codespace is running.
        // Display available port configuration.
        if let Some(ports) = codespace
            .get("runtime_constraints")
            .and_then(|r| r.get("allowed_port_privacy_settings"))
            .and_then(Value::as_array)
        {
            let mut tp = TablePrinter::new(ios);
            for port in ports {
                if let Some(port_str) = port.as_str() {
                    tp.add_row(vec![port_str.to_string()]);
                }
            }
            let output = tp.render();
            ios_println!(ios, "{output}");
        } else {
            ios_eprintln!(
                ios,
                "No port information available (codespace may not be running)"
            );
        }

        Ok(())
    }
}
