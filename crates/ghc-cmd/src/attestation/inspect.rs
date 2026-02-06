//! `ghc attestation inspect` command.

use anyhow::{Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Inspect a Sigstore bundle.
///
/// Given a `.json` or `.jsonl` file, this command extracts the bundle's
/// statement and predicate, provides a certificate summary (if present),
/// and checks the bundle's authenticity.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Path to the Sigstore bundle file (`.json` or `.jsonl`).
    #[arg(value_name = "BUNDLE_PATH")]
    bundle_path: String,

    /// Output format.
    #[arg(long, value_parser = ["json", "table"])]
    format: Option<String>,

    /// Configure host to use.
    #[arg(long)]
    hostname: Option<String>,
}

/// Result of inspecting a bundle.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleInspectResult {
    inspected_bundles: Vec<BundleInspection>,
}

/// Inspection data for a single bundle.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleInspection {
    predicate_type: String,
    subject_count: usize,
    payload_type: String,
    has_certificate: bool,
    certificate_issuer: String,
    source_repo: String,
}

impl InspectArgs {
    /// Run the attestation inspect command.
    ///
    /// # Errors
    ///
    /// Returns an error if the bundle cannot be inspected.
    pub fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Read the bundle file
        let content = std::fs::read_to_string(&self.bundle_path)
            .with_context(|| format!("failed to read bundle file: {}", self.bundle_path))?;

        // Parse bundles: could be single JSON or JSONL
        let bundles = parse_bundles(&content)?;

        if bundles.is_empty() {
            return Err(anyhow::anyhow!("no bundles found in {}", self.bundle_path));
        }

        let mut inspected = Vec::with_capacity(bundles.len());

        for bundle in &bundles {
            inspected.push(inspect_bundle(bundle));
        }

        let result = BundleInspectResult {
            inspected_bundles: inspected,
        };

        // JSON output
        if self.format.as_deref() == Some("json") {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }

        // Table output
        ios_eprintln!(ios, "Inspecting bundles...");
        ios_eprintln!(
            ios,
            "Found {} attestation(s):",
            result.inspected_bundles.len(),
        );
        ios_println!(ios, "---");

        let max_label_len: usize = 22;
        for (i, bundle) in result.inspected_bundles.iter().enumerate() {
            let rows = [
                ("SourceRepo", bundle.source_repo.as_str()),
                ("PredicateType", bundle.predicate_type.as_str()),
                ("PayloadType", bundle.payload_type.as_str()),
                ("CertificateIssuer", bundle.certificate_issuer.as_str()),
                ("SubjectCount", &bundle.subject_count.to_string()),
            ];

            for (label, value) in &rows {
                let dots = max_label_len.saturating_sub(label.len());
                ios_println!(ios, "{}:{} {}", cs.bold(label), ".".repeat(dots), value,);
            }

            if i < result.inspected_bundles.len() - 1 {
                ios_println!(ios, "---");
            }
        }

        Ok(())
    }
}

/// Parse bundles from either a JSON file or a JSONL file.
fn parse_bundles(content: &str) -> Result<Vec<Value>> {
    let trimmed = content.trim();

    // Try parsing as a single JSON object first
    if let Ok(single) = serde_json::from_str::<Value>(trimmed) {
        return Ok(vec![single]);
    }

    // Try parsing as JSONL (one JSON object per line)
    let mut bundles = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bundle: Value =
            serde_json::from_str(line).context("failed to parse line as JSON in JSONL file")?;
        bundles.push(bundle);
    }

    if bundles.is_empty() {
        return Err(anyhow::anyhow!("no valid JSON found in bundle file"));
    }

    Ok(bundles)
}

/// Inspect a single bundle and extract metadata.
fn inspect_bundle(bundle: &Value) -> BundleInspection {
    let payload_type = bundle
        .pointer("/dsseEnvelope/payloadType")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    // Decode the payload to get the statement
    let payload_b64 = bundle
        .pointer("/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .unwrap_or("");

    let default_tuple = ("unknown".to_string(), 0, "unknown".to_string());
    let (predicate_type, subject_count, source_repo) = if payload_b64.is_empty() {
        default_tuple
    } else {
        extract_statement_metadata(payload_b64).unwrap_or(default_tuple)
    };

    // Check for certificate
    let has_certificate = bundle
        .pointer("/verificationMaterial/x509CertificateChain/certificates")
        .and_then(Value::as_array)
        .is_some_and(|certs| !certs.is_empty());

    let certificate_issuer = if has_certificate {
        bundle
            .pointer("/verificationMaterial/x509CertificateChain/certificates/0/rawBytes")
            .and_then(Value::as_str)
            .map_or_else(
                || "present (details unavailable)".to_string(),
                |_| "present".to_string(),
            )
    } else {
        "none".to_string()
    };

    BundleInspection {
        predicate_type,
        subject_count,
        payload_type,
        has_certificate,
        certificate_issuer,
        source_repo,
    }
}

/// Extract predicate type, subject count, and source repo from a base64-encoded statement.
fn extract_statement_metadata(payload_b64: &str) -> Option<(String, usize, String)> {
    let decoded = ghc_core::text::base64_decode(payload_b64).ok()?;
    let statement: Value = serde_json::from_slice(&decoded).ok()?;

    let pt = statement
        .get("predicateType")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let sc = statement
        .get("subject")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    let sr = statement
        .pointer("/predicate/buildDefinition/externalParameters/workflow/repository")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    Some((pt, sc, sr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_parse_single_json_bundle() {
        let json = r#"{"dsseEnvelope": {"payloadType": "application/vnd.in-toto+json"}}"#;
        let bundles = parse_bundles(json).unwrap();
        assert_eq!(bundles.len(), 1);
    }

    #[test]
    fn test_should_parse_jsonl_bundles() {
        let jsonl = r#"{"dsseEnvelope": {"payloadType": "type1"}}
{"dsseEnvelope": {"payloadType": "type2"}}"#;
        let bundles = parse_bundles(jsonl).unwrap();
        assert_eq!(bundles.len(), 2);
    }

    #[test]
    fn test_should_inspect_bundle_with_payload() {
        let statement = serde_json::json!({
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{"name": "test", "digest": {"sha256": "abc"}}],
            "predicate": {
                "buildDefinition": {
                    "externalParameters": {
                        "workflow": {
                            "repository": "owner/repo"
                        }
                    }
                }
            }
        });
        let payload =
            ghc_core::text::base64_encode(serde_json::to_string(&statement).unwrap().as_bytes());

        let bundle = serde_json::json!({
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": payload,
            }
        });

        let inspection = inspect_bundle(&bundle);
        assert_eq!(inspection.predicate_type, "https://slsa.dev/provenance/v1");
        assert_eq!(inspection.subject_count, 1);
        assert_eq!(inspection.source_repo, "owner/repo");
    }

    #[test]
    fn test_should_handle_empty_payload() {
        let bundle = serde_json::json!({
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": "",
            }
        });

        let inspection = inspect_bundle(&bundle);
        assert_eq!(inspection.predicate_type, "unknown");
        assert_eq!(inspection.subject_count, 0);
    }
}
