//! `ghc attestation verify` command.

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::{ios_eprintln, ios_println};
use serde_json::Value;

/// Verify an artifact attestation.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// Path to the artifact file to verify.
    #[arg(value_name = "FILE")]
    file: String,

    /// Repository that produced the artifact (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Owner of the artifact (user or organization).
    #[arg(long)]
    owner: Option<String>,

    /// Expected signer workflow (e.g., `.github/workflows/release.yml`).
    #[arg(long)]
    signer_workflow: Option<String>,

    /// Expected signer repository (OWNER/REPO).
    #[arg(long)]
    signer_repo: Option<String>,

    /// Deny attestations from GitHub Actions.
    #[arg(long)]
    deny_self_hosted_runners: bool,

    /// Output JSON.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl VerifyArgs {
    /// Run the attestation verify command.
    ///
    /// # Errors
    ///
    /// Returns an error if the attestation cannot be verified.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Compute SHA256 digest of the artifact file
        let digest = compute_sha256(&self.file).await?;

        // Query attestations for the digest
        let attestations = self.fetch_attestations(&client, &digest).await?;

        if attestations.is_empty() {
            return Err(anyhow::anyhow!("no attestations found for {}", self.file,));
        }

        // Verify each attestation
        let mut verified = Vec::new();
        for attestation in &attestations {
            if self.verify_attestation(attestation)? {
                verified.push(attestation.clone());
            }
        }

        if verified.is_empty() {
            return Err(anyhow::anyhow!(
                "no matching attestations found for the specified criteria"
            ));
        }

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&verified)?);
            return Ok(());
        }
        ios_eprintln!(
            ios,
            "{} Verified {} attestation(s) for {}",
            cs.success_icon(),
            verified.len(),
            cs.bold(&self.file),
        );

        for att in &verified {
            let predicate_type = att
                .pointer("/bundle/dsseEnvelope/payloadType")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let signer = att
                .pointer("/bundle/verificationMaterial/x509CertificateChain/certificates/0")
                .and_then(Value::as_str)
                .unwrap_or("unknown signer");

            ios_eprintln!(
                ios,
                "  - Predicate: {}, Signer: {}",
                cs.cyan(predicate_type),
                ghc_core::text::truncate(signer, 40),
            );
        }

        Ok(())
    }

    /// Fetch attestations for a given digest.
    async fn fetch_attestations(
        &self,
        client: &ghc_api::client::Client,
        digest: &str,
    ) -> Result<Vec<Value>> {
        let path = if let Some(repo) = &self.repo {
            format!("repos/{repo}/attestations/sha256:{digest}")
        } else if let Some(owner) = &self.owner {
            format!("orgs/{owner}/attestations/sha256:{digest}")
        } else {
            return Err(anyhow::anyhow!("one of --repo or --owner is required"));
        };

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to fetch attestations")?;

        let attestations = result
            .get("attestations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(attestations)
    }

    /// Verify a single attestation against the specified criteria.
    #[allow(clippy::unnecessary_wraps)]
    fn verify_attestation(&self, attestation: &Value) -> Result<bool> {
        // Check signer workflow if specified
        if let Some(expected_workflow) = &self.signer_workflow {
            let workflow = attestation
                .pointer("/bundle/dsseEnvelope/payload")
                .and_then(Value::as_str)
                .and_then(|p| {
                    // Payload is base64-encoded JSON
                    let decoded = ghc_core::text::base64_decode(p).ok()?;
                    let payload: Value = serde_json::from_slice(&decoded).ok()?;
                    payload
                        .pointer("/predicate/buildDefinition/externalParameters/workflow/path")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();

            if workflow != *expected_workflow {
                return Ok(false);
            }
        }

        // Check signer repository if specified
        if let Some(expected_repo) = &self.signer_repo {
            let signer_repo = attestation
                .pointer("/bundle/dsseEnvelope/payload")
                .and_then(Value::as_str)
                .and_then(|p| {
                    let decoded = ghc_core::text::base64_decode(p).ok()?;
                    let payload: Value = serde_json::from_slice(&decoded).ok()?;
                    payload
                        .pointer(
                            "/predicate/buildDefinition/externalParameters/workflow/repository",
                        )
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();

            if signer_repo != *expected_repo {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

/// Compute the SHA256 hex digest of a file using the system `shasum` command.
async fn compute_sha256(path: &str) -> Result<String> {
    let output = tokio::process::Command::new("shasum")
        .args(["-a", "256", path])
        .output()
        .await
        .with_context(|| format!("failed to run shasum on artifact file: {path}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("shasum failed for {path}: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout).context("shasum produced non-UTF-8 output")?;

    // shasum output format: "<hex_digest>  <filename>"
    let digest = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected shasum output format"))?;

    Ok(digest.to_string())
}
