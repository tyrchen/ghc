//! API command (`ghc api`).
//!
//! Make an authenticated GitHub API request.

use anyhow::Context;
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Make an authenticated GitHub API request.
///
/// Provides a generic interface for making REST or GraphQL requests
/// to the GitHub API, similar to `curl` but with authentication.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Make additional HTTP requests to fetch all pages of results.
    #[arg(long)]
    paginate: bool,

    /// Use jq expression to filter output.
    #[arg(short = 'q', long)]
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

    /// Do not print the response body.
    #[arg(long)]
    silent: bool,

    /// Wrap paginated results in a JSON array.
    #[arg(long)]
    slurp: bool,
}

impl ApiArgs {
    /// Run the api command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        self.validate_flags()?;

        let hostname = self.hostname.as_deref().unwrap_or("github.com");
        let client = factory.api_client(hostname)?;
        let ios = &factory.io;

        let method = self.method.to_uppercase();
        let method: reqwest::Method = method
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid HTTP method: {}", self.method))?;

        let body = self.build_body()?;

        if self.verbose {
            ios_eprintln!(ios, "> {} /{}", method, self.endpoint);
            for h in &self.header {
                ios_eprintln!(ios, "> {h}");
            }
            ios_eprintln!(ios, "");
        }

        if self.paginate {
            self.run_paginated(&client, &method, body.as_ref(), factory)
                .await
        } else {
            self.run_single(&client, &method, body.as_ref(), factory)
                .await
        }
    }

    /// Run a single (non-paginated) API request.
    async fn run_single(
        &self,
        client: &ghc_api::client::Client,
        method: &reqwest::Method,
        body: Option<&Value>,
        factory: &crate::factory::Factory,
    ) -> anyhow::Result<()> {
        let ios = &factory.io;

        let result: Value = client
            .rest(method.clone(), &self.endpoint, body)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.output_result(&result, ios)
    }

    /// Run paginated API requests, fetching all pages.
    async fn run_paginated(
        &self,
        client: &ghc_api::client::Client,
        method: &reqwest::Method,
        body: Option<&Value>,
        factory: &crate::factory::Factory,
    ) -> anyhow::Result<()> {
        let ios = &factory.io;

        // Add per_page parameter if not already present
        let mut endpoint = self.endpoint.clone();
        if !endpoint.contains("per_page=") {
            let separator = if endpoint.contains('?') { "&" } else { "?" };
            endpoint = format!("{endpoint}{separator}per_page=100");
        }

        let mut all_results: Vec<Value> = Vec::new();
        let mut current_endpoint = endpoint;

        loop {
            let page: ghc_api::client::RestPage<Value> = client
                .rest_with_next(method.clone(), &current_endpoint, body)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            if self.slurp {
                // Collect for slurp mode
                all_results.push(page.data);
            } else {
                // Output each page immediately
                self.output_result(&page.data, ios)?;
            }

            match page.next_url {
                Some(next) => current_endpoint = next,
                None => break,
            }
        }

        if self.slurp {
            let combined = Value::Array(all_results);
            self.output_result(&combined, ios)?;
        }

        Ok(())
    }

    /// Output the API result, applying jq filter if specified.
    fn output_result(
        &self,
        result: &Value,
        ios: &ghc_core::iostreams::IOStreams,
    ) -> anyhow::Result<()> {
        if self.silent {
            return Ok(());
        }

        if let Some(ref jq_expr) = self.jq {
            let filtered = apply_jq_filter(result, jq_expr)?;
            ios_println!(ios, "{}", format_output(&filtered, ios.is_stdout_tty()));
        } else {
            ios_println!(ios, "{}", format_output(result, ios.is_stdout_tty()));
        }

        Ok(())
    }

    /// Validate flag combinations.
    fn validate_flags(&self) -> anyhow::Result<()> {
        if self.paginate && !self.method.eq_ignore_ascii_case("GET") && self.endpoint != "graphql" {
            return Err(anyhow::anyhow!(
                "the `--paginate` option is not supported for non-GET requests"
            ));
        }

        if self.paginate && self.input.is_some() {
            return Err(anyhow::anyhow!(
                "the `--paginate` option is not supported with `--input`"
            ));
        }

        if self.slurp && !self.paginate {
            return Err(anyhow::anyhow!(
                "`--paginate` required when passing `--slurp`"
            ));
        }

        if self.slurp && self.jq.is_some() {
            return Err(anyhow::anyhow!(
                "the `--slurp` option is not supported with `--jq`"
            ));
        }

        let exclusive_count =
            u8::from(self.verbose) + u8::from(self.silent) + u8::from(self.jq.is_some());
        if exclusive_count > 1 {
            return Err(anyhow::anyhow!(
                "only one of `--verbose`, `--silent`, or `--jq` may be used"
            ));
        }

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
            let content = std::fs::read_to_string(input_path)
                .with_context(|| format!("failed to read input file: {input_path}"))?;
            let parsed: Value = serde_json::from_str(&content)
                .with_context(|| format!("failed to parse JSON from {input_path}"))?;
            return Ok(Some(parsed));
        }

        Ok(Some(Value::Object(body)))
    }
}

/// Apply a jq-style filter expression to a JSON value.
///
/// Supports common jq path expressions:
/// - `.field` - access object field
/// - `.[n]` - access array element
/// - `.[]` - iterate array elements
/// - `.field1.field2` - nested access
/// - `.[].field` - map over array elements
fn apply_jq_filter(value: &Value, expr: &str) -> anyhow::Result<Value> {
    let expr = expr.trim();

    // Identity filter
    if expr == "." {
        return Ok(value.clone());
    }

    // Strip leading dot
    let expr = expr.strip_prefix('.').unwrap_or(expr);

    // Handle array iteration: .[]
    if expr == "[]" {
        if let Some(arr) = value.as_array() {
            return Ok(Value::Array(arr.clone()));
        }
        return Err(anyhow::anyhow!("cannot iterate over non-array value"));
    }

    // Handle .[].field pattern
    if let Some(rest) = expr.strip_prefix("[].") {
        if let Some(arr) = value.as_array() {
            let results: Vec<Value> = arr
                .iter()
                .filter_map(|item| apply_jq_filter(item, &format!(".{rest}")).ok())
                .collect();
            return Ok(Value::Array(results));
        }
        return Err(anyhow::anyhow!("cannot iterate over non-array value"));
    }

    // Handle array index: .[n]
    if expr.starts_with('[')
        && let Some(idx_str) = expr.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
        && let Ok(idx) = idx_str.parse::<usize>()
    {
        return value
            .get(idx)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("array index {idx} out of bounds"));
    }

    // Handle field access chain: field1.field2
    let mut current = value;
    for part in expr.split('.') {
        if part.is_empty() {
            continue;
        }

        // Check for array index like field[0]
        if let Some((field, rest)) = part.split_once('[') {
            current = current
                .get(field)
                .ok_or_else(|| anyhow::anyhow!("field '{field}' not found"))?;

            if let Some(idx_str) = rest.strip_suffix(']')
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                current = current
                    .get(idx)
                    .ok_or_else(|| anyhow::anyhow!("array index {idx} out of bounds"))?;
            }
        } else {
            current = current
                .get(part)
                .ok_or_else(|| anyhow::anyhow!("field '{part}' not found"))?;
        }
    }

    Ok(current.clone())
}

/// Format a JSON value for output.
fn format_output(value: &Value, pretty: bool) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(arr) if pretty => {
            // For arrays of strings from jq, output one per line
            let all_strings = arr.iter().all(Value::is_string);
            if all_strings {
                arr.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                serde_json::to_string_pretty(value).unwrap_or_default()
            }
        }
        _ if pretty => serde_json::to_string_pretty(value).unwrap_or_default(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_apply_jq_identity() {
        let val = serde_json::json!({"a": 1});
        let result = apply_jq_filter(&val, ".").unwrap();
        assert_eq!(result, val);
    }

    #[test]
    fn test_should_apply_jq_field_access() {
        let val = serde_json::json!({"name": "test", "count": 42});
        let result = apply_jq_filter(&val, ".name").unwrap();
        assert_eq!(result, Value::String("test".into()));
    }

    #[test]
    fn test_should_apply_jq_nested_access() {
        let val = serde_json::json!({"a": {"b": {"c": "deep"}}});
        let result = apply_jq_filter(&val, ".a.b.c").unwrap();
        assert_eq!(result, Value::String("deep".into()));
    }

    #[test]
    fn test_should_apply_jq_array_map() {
        let val = serde_json::json!([
            {"title": "issue 1"},
            {"title": "issue 2"},
            {"title": "issue 3"}
        ]);
        let result = apply_jq_filter(&val, ".[].title").unwrap();
        assert_eq!(
            result,
            Value::Array(vec![
                Value::String("issue 1".into()),
                Value::String("issue 2".into()),
                Value::String("issue 3".into()),
            ]),
        );
    }

    #[test]
    fn test_should_apply_jq_array_index() {
        let val = serde_json::json!(["a", "b", "c"]);
        let result = apply_jq_filter(&val, ".[1]").unwrap();
        assert_eq!(result, Value::String("b".into()));
    }

    #[test]
    fn test_should_return_error_for_missing_field() {
        let val = serde_json::json!({"a": 1});
        let result = apply_jq_filter(&val, ".b");
        assert!(result.is_err());
    }

    #[test]
    fn test_should_format_string_output() {
        let val = Value::String("hello".into());
        assert_eq!(format_output(&val, true), "hello");
    }

    #[test]
    fn test_should_format_array_of_strings() {
        let val = serde_json::json!(["a", "b", "c"]);
        assert_eq!(format_output(&val, true), "a\nb\nc");
    }

    #[test]
    fn test_should_validate_paginate_with_non_get() {
        let args = ApiArgs {
            endpoint: "repos/owner/repo".into(),
            method: "POST".into(),
            field: vec![],
            raw_field: vec![],
            header: vec![],
            include: false,
            paginate: true,
            jq: None,
            hostname: None,
            input: None,
            verbose: false,
            silent: false,
            slurp: false,
        };
        assert!(args.validate_flags().is_err());
    }

    #[test]
    fn test_should_validate_slurp_without_paginate() {
        let args = ApiArgs {
            endpoint: "repos/owner/repo".into(),
            method: "GET".into(),
            field: vec![],
            raw_field: vec![],
            header: vec![],
            include: false,
            paginate: false,
            jq: None,
            hostname: None,
            input: None,
            verbose: false,
            silent: false,
            slurp: true,
        };
        assert!(args.validate_flags().is_err());
    }

    #[test]
    fn test_should_validate_verbose_and_silent_exclusive() {
        let args = ApiArgs {
            endpoint: "repos/owner/repo".into(),
            method: "GET".into(),
            field: vec![],
            raw_field: vec![],
            header: vec![],
            include: false,
            paginate: false,
            jq: None,
            hostname: None,
            input: None,
            verbose: true,
            silent: true,
            slurp: false,
        };
        assert!(args.validate_flags().is_err());
    }
}
