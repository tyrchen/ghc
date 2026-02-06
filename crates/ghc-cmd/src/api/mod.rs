//! API command (`ghc api`).
//!
//! Make an authenticated GitHub API request.

use clap::Args;
use serde_json::Value;

/// Make an authenticated GitHub API request.
///
/// Provides a generic interface for making REST or GraphQL requests
/// to the GitHub API, similar to `curl` but with authentication.
#[derive(Debug, Args)]
pub struct ApiArgs {
    /// The endpoint path (e.g., `repos/{owner}/{repo}/issues`).
    endpoint: String,

    /// The HTTP method to use.
    #[arg(short = 'X', long, default_value = "GET")]
    method: String,

    /// Request body (JSON string or @file).
    #[arg(short = 'f', long)]
    field: Vec<String>,

    /// Raw request body fields.
    #[arg(short = 'F', long = "raw-field")]
    raw_field: Vec<String>,

    /// Add a HTTP request header.
    #[arg(short = 'H', long)]
    header: Vec<String>,

    /// Include HTTP response status line and headers in output.
    #[arg(short, long)]
    include: bool,

    /// Add pagination parameters to the request.
    #[arg(long)]
    paginate: bool,

    /// Use jq expression to filter output.
    #[arg(short, long)]
    jq: Option<String>,

    /// The hostname for the request.
    #[arg(long)]
    hostname: Option<String>,

    /// Input file for the request body.
    #[arg(long)]
    input: Option<String>,

    /// Print verbose request/response info.
    #[arg(long)]
    verbose: bool,
}

impl ApiArgs {
    /// Run the api command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        let hostname = self.hostname.as_deref().unwrap_or("github.com");
        let client = factory.api_client(hostname)?;

        let method = self.method.to_uppercase();
        let method: reqwest::Method = method
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid HTTP method: {}", self.method))?;

        let body = self.build_body()?;

        let result: Value = client
            .rest(method, &self.endpoint, body.as_ref())
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let output = serde_json::to_string_pretty(&result)?;
        println!("{output}");

        Ok(())
    }

    fn build_body(&self) -> anyhow::Result<Option<Value>> {
        if self.field.is_empty() && self.raw_field.is_empty() && self.input.is_none() {
            return Ok(None);
        }

        let mut body = serde_json::Map::new();

        for field in &self.field {
            if let Some((key, value)) = field.split_once('=') {
                // Try to parse as JSON value, fall back to string
                let json_value: Value = serde_json::from_str(value)
                    .unwrap_or_else(|_| Value::String(value.to_string()));
                body.insert(key.to_string(), json_value);
            }
        }

        for field in &self.raw_field {
            if let Some((key, value)) = field.split_once('=') {
                body.insert(key.to_string(), Value::String(value.to_string()));
            }
        }

        if let Some(ref input_path) = self.input {
            if input_path == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                let parsed: Value = serde_json::from_str(&buf)?;
                return Ok(Some(parsed));
            }
            let content = std::fs::read_to_string(input_path)?;
            let parsed: Value = serde_json::from_str(&content)?;
            return Ok(Some(parsed));
        }

        Ok(Some(Value::Object(body)))
    }
}
