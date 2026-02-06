//! `ghc attestation download` command.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Minimum allowed limit value.
const MIN_LIMIT: u32 = 1;

/// Maximum allowed limit value.
const MAX_LIMIT: u32 = 1000;

/// Default limit for attestation fetching.
const DEFAULT_LIMIT: u32 = 30;

/// Download an artifact's attestations for offline use.
///
/// Downloads attestation bundles associated with an artifact and writes
/// them to a JSONL file named after the artifact's digest.
#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Path to the artifact file, or `oci://<image-uri>`.
    #[arg(value_name = "FILE")]
    artifact_path: String,

    /// GitHub organization to scope attestation lookup by.
    #[arg(short = 'o', long)]
    owner: Option<String>,

    /// Repository name in the format OWNER/REPO.
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Filter attestations by predicate type.
    #[arg(long)]
    predicate_type: Option<String>,

    /// The algorithm used to compute a digest of the artifact.
    #[arg(short = 'd', long, default_value = "sha256", value_parser = ["sha256", "sha512"])]
    digest_alg: String,

    /// Maximum number of attestations to fetch.
    #[arg(short = 'L', long, default_value_t = DEFAULT_LIMIT)]
    limit: u32,

    /// Configure host to use.
    #[arg(long)]
    hostname: Option<String>,
}

impl DownloadArgs {
    /// Run the attestation download command.
    ///
    /// # Errors
    ///
    /// Returns an error if the attestations cannot be downloaded.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        // Validate flags
        if self.owner.is_none() && self.repo.is_none() {
            return Err(anyhow::anyhow!("one of --owner or --repo is required"));
        }
        if self.owner.is_some() && self.repo.is_some() {
            return Err(anyhow::anyhow!("--owner and --repo are mutually exclusive"));
        }
        if self.limit < MIN_LIMIT || self.limit > MAX_LIMIT {
            return Err(anyhow::anyhow!(
                "limit {} not allowed, must be between {MIN_LIMIT} and {MAX_LIMIT}",
                self.limit,
            ));
        }

        let hostname = self.hostname.as_deref().unwrap_or("github.com");
        let client = factory.api_client(hostname)?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Compute digest of the artifact
        let digest = compute_digest(&self.artifact_path, &self.digest_alg).await?;
        let digest_with_alg = format!("{}:{digest}", self.digest_alg);

        ios_eprintln!(
            ios,
            "Downloading trusted metadata for artifact {}",
            self.artifact_path,
        );

        // Fetch attestations
        let path = if let Some(ref repo) = self.repo {
            format!("repos/{repo}/attestations/{digest_with_alg}")
        } else if let Some(ref owner) = self.owner {
            format!("orgs/{owner}/attestations/{digest_with_alg}")
        } else {
            return Err(anyhow::anyhow!("one of --owner or --repo is required"));
        };

        let query_path = format!("{path}?per_page={}", self.limit);

        let result: Value = client
            .rest(reqwest::Method::GET, &query_path, None::<&Value>)
            .await
            .context("failed to fetch attestations")?;

        let attestations = result
            .get("attestations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if attestations.is_empty() {
            ios_println!(ios, "No attestations found for {}", self.artifact_path);
            return Ok(());
        }

        // Filter by predicate type if specified
        let attestations = if let Some(ref pred_type) = self.predicate_type {
            attestations
                .into_iter()
                .filter(|att| {
                    att.pointer("/bundle/dsseEnvelope/payload")
                        .and_then(Value::as_str)
                        .and_then(|p| {
                            let decoded = ghc_core::text::base64_decode(p).ok()?;
                            let payload: Value = serde_json::from_slice(&decoded).ok()?;
                            payload
                                .get("predicateType")
                                .and_then(Value::as_str)
                                .map(|pt| pt == pred_type)
                        })
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>()
        } else {
            attestations
        };

        // Write to JSONL file
        let file_name = create_jsonl_filename(&digest_with_alg);
        let mut output = String::new();
        for att in &attestations {
            if let Some(bundle) = att.get("bundle") {
                let line =
                    serde_json::to_string(bundle).context("failed to serialize attestation")?;
                output.push_str(&line);
                output.push('\n');
            }
        }

        std::fs::write(&file_name, &output)
            .with_context(|| format!("failed to write attestation file: {file_name}"))?;

        ios_println!(
            ios,
            "Wrote attestations to file {file_name}.\nAny previous content has been overwritten",
        );
        ios_println!(ios, "");
        ios_eprintln!(
            ios,
            "{}",
            cs.success(&format!(
                "The trusted metadata is now available at {file_name}"
            )),
        );

        Ok(())
    }
}

/// Compute a hex digest of a file using the specified algorithm.
async fn compute_digest(path: &str, alg: &str) -> Result<String> {
    // Check for OCI URI
    if path.starts_with("oci://") {
        return Err(anyhow::anyhow!(
            "OCI image URIs are not yet supported in this implementation"
        ));
    }

    // Verify file exists
    if !Path::new(path).exists() {
        return Err(anyhow::anyhow!("artifact file not found: {path}"));
    }

    let alg_flag = match alg {
        "sha256" => "256",
        "sha512" => "512",
        _ => return Err(anyhow::anyhow!("unsupported digest algorithm: {alg}")),
    };

    let output = tokio::process::Command::new("shasum")
        .args(["-a", alg_flag, path])
        .output()
        .await
        .with_context(|| format!("failed to run shasum on artifact file: {path}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("shasum failed for {path}: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout).context("shasum produced non-UTF-8 output")?;

    let digest = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected shasum output format"))?;

    Ok(digest.to_string())
}

/// Create a JSONL filename from a digest string.
///
/// On Windows, colons are replaced with dashes.
fn create_jsonl_filename(digest_with_alg: &str) -> String {
    let sanitized = if cfg!(target_os = "windows") {
        digest_with_alg.replace(':', "-")
    } else {
        digest_with_alg.to_string()
    };
    format!("{sanitized}.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_create_jsonl_filename() {
        let result = create_jsonl_filename("sha256:abcdef");
        assert!(
            std::path::Path::new(&result)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        );
        if cfg!(target_os = "windows") {
            assert_eq!(result, "sha256-abcdef.jsonl");
        } else {
            assert_eq!(result, "sha256:abcdef.jsonl");
        }
    }

    #[test]
    fn test_should_validate_limit_range() {
        const { assert!(MIN_LIMIT <= DEFAULT_LIMIT) };
        const { assert!(DEFAULT_LIMIT <= MAX_LIMIT) };
    }
}
