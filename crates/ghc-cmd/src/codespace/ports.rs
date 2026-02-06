//! `ghc codespace ports` command.

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Manage codespace port forwarding.
#[derive(Debug, Args)]
pub struct PortsArgs {
    /// Name of the codespace.
    #[arg(short, long, global = true)]
    codespace: Option<String>,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',', global = true)]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,

    /// Subcommand.
    #[command(subcommand)]
    command: Option<PortsCommand>,
}

/// Port subcommands.
#[derive(Debug, Subcommand)]
pub enum PortsCommand {
    /// Forward ports from a codespace.
    Forward(ForwardArgs),
    /// Set port visibility.
    Visibility(VisibilityArgs),
}

/// Forward ports from a codespace to local machine.
#[derive(Debug, Args)]
pub struct ForwardArgs {
    /// Port mapping in the form REMOTE:LOCAL (e.g. 8080:8080).
    #[arg(required = true, num_args = 1..)]
    ports: Vec<String>,
}

/// Set port visibility for a codespace.
#[derive(Debug, Args)]
pub struct VisibilityArgs {
    /// Port visibility in the form PORT:VISIBILITY (e.g. 80:public, 3000:private, 8080:org).
    #[arg(required = true, num_args = 1..)]
    mappings: Vec<String>,
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

        match &self.command {
            Some(PortsCommand::Forward(args)) => self.run_forward(codespace_name, args).await,
            Some(PortsCommand::Visibility(args)) => {
                self.run_visibility(factory, codespace_name, args).await
            }
            None => self.run_list(factory, codespace_name).await,
        }
    }

    /// List ports for a codespace.
    async fn run_list(
        &self,
        factory: &crate::factory::Factory,
        codespace_name: &str,
    ) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let path = format!("user/codespaces/{codespace_name}");
        let codespace: Value = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to fetch codespace")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &codespace,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let cs = ios.color_scheme();

        // Try to get runtime status ports info
        if let Some(ports) = codespace
            .get("runtime_status")
            .and_then(|r| r.get("forwarded_ports"))
            .and_then(Value::as_array)
        {
            if ports.is_empty() {
                ios_eprintln!(
                    ios,
                    "No forwarded ports for codespace {}",
                    cs.bold(codespace_name)
                );
                return Ok(());
            }

            let mut tp =
                TablePrinter::new(ios).with_headers(&["SOURCE PORT", "VISIBILITY", "BROWSE URL"]);

            for port in ports {
                let source = port
                    .get("source_port")
                    .and_then(Value::as_u64)
                    .map_or_else(|| "-".to_string(), |p| p.to_string());
                let visibility = port
                    .get("visibility")
                    .and_then(Value::as_str)
                    .unwrap_or("private");
                let browse_url = port
                    .get("browse_url")
                    .and_then(Value::as_str)
                    .unwrap_or("-");
                tp.add_row(vec![source, visibility.to_string(), browse_url.to_string()]);
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

    /// Forward ports from a codespace to local machine via SSH tunneling.
    async fn run_forward(&self, codespace_name: &str, args: &ForwardArgs) -> Result<()> {
        // Build: gh codespace ssh -c NAME -L local:localhost:remote ... -- sleep infinity
        let mut cmd_args = vec![
            "codespace".to_string(),
            "ssh".to_string(),
            "-c".to_string(),
            codespace_name.to_string(),
        ];

        for mapping in &args.ports {
            let parts: Vec<&str> = mapping.split(':').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!(
                    "invalid port mapping: {mapping} (expected REMOTE:LOCAL)"
                ));
            }
            let remote_port = parts[0];
            let local_port = parts[1];
            remote_port
                .parse::<u16>()
                .with_context(|| format!("invalid remote port: {remote_port}"))?;
            local_port
                .parse::<u16>()
                .with_context(|| format!("invalid local port: {local_port}"))?;
            cmd_args.push("-L".to_string());
            cmd_args.push(format!("{local_port}:localhost:{remote_port}"));
        }

        cmd_args.push("--".to_string());
        cmd_args.push("sleep".to_string());
        cmd_args.push("infinity".to_string());

        let status = tokio::process::Command::new("gh")
            .args(&cmd_args)
            .status()
            .await
            .context("failed to start port forwarding via gh codespace ssh")?;

        if !status.success() {
            return Err(anyhow::anyhow!("port forwarding exited with an error"));
        }

        Ok(())
    }

    /// Set port visibility for a codespace.
    async fn run_visibility(
        &self,
        factory: &crate::factory::Factory,
        codespace_name: &str,
        args: &VisibilityArgs,
    ) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        for mapping in &args.mappings {
            let parts: Vec<&str> = mapping.split(':').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!(
                    "invalid visibility mapping: {mapping} (expected PORT:VISIBILITY)"
                ));
            }
            let port_str = parts[0];
            let visibility = parts[1];

            let port: u16 = port_str
                .parse()
                .with_context(|| format!("invalid port number: {port_str}"))?;

            match visibility {
                "public" | "private" | "org" => {}
                _ => {
                    return Err(anyhow::anyhow!(
                        "invalid visibility: {visibility} (must be public, private, or org)"
                    ));
                }
            }

            let path = format!("user/codespaces/{codespace_name}/ports/{port}/visibility");
            let body = serde_json::json!({
                "visibility": visibility,
            });
            client
                .rest::<Value>(reqwest::Method::PATCH, &path, Some(&body))
                .await
                .with_context(|| format!("failed to set visibility for port {port}"))?;

            ios_eprintln!(
                ios,
                "{} Set port {} to {}",
                cs.success_icon(),
                cs.bold(&port.to_string()),
                cs.bold(visibility),
            );
        }

        Ok(())
    }
}
