//! `ghc release verify` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// Verify the attestation for a release.
///
/// Checks that the specified release (or the latest release, if no tag is given)
/// has a valid attestation. Fetches the attestation and prints metadata about
/// all assets referenced, including their digests.
#[derive(Debug, Args)]
pub struct VerifyArgs {
    /// The release tag to verify. Uses latest release if not specified.
    #[arg(value_name = "TAG")]
    tag: Option<String>,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Option<String>,

    /// Output JSON.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl VerifyArgs {
    /// Run the release verify command.
    ///
    /// # Errors
    ///
    /// Returns an error if the release attestation cannot be verified.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo_str = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo_str).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Determine the tag name
        let tag_name = if let Some(ref tag) = self.tag {
            tag.clone()
        } else {
            // Fetch latest release
            let path = format!("repos/{}/{}/releases/latest", repo.owner(), repo.name(),);
            let release: Value = client
                .rest(reqwest::Method::GET, &path, None::<&Value>)
                .await
                .context("failed to fetch latest release")?;
            release
                .get("tag_name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("no tag_name in latest release"))?
                .to_string()
        };

        // Fetch the git ref SHA for the tag
        let ref_path = format!(
            "repos/{}/{}/git/ref/tags/{}",
            repo.owner(),
            repo.name(),
            tag_name,
        );
        let ref_data: Value = client
            .rest(reqwest::Method::GET, &ref_path, None::<&Value>)
            .await
            .context("failed to fetch tag ref")?;

        let sha = ref_data
            .pointer("/object/sha")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("failed to resolve tag {tag_name} to a SHA"))?;

        let digest = format!("sha1:{sha}");

        // Fetch attestations for the release tag SHA
        let att_path = format!(
            "repos/{}/{}/attestations/{digest}?per_page=100",
            repo.owner(),
            repo.name(),
        );
        let att_result: Value = client
            .rest(reqwest::Method::GET, &att_path, None::<&Value>)
            .await
            .with_context(|| format!("no attestations for tag {tag_name} ({digest})"))?;

        let attestations = att_result
            .get("attestations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // Filter attestations by tag name (look in the payload for matching tag)
        let filtered: Vec<&Value> = attestations
            .iter()
            .filter(|att| {
                att.pointer("/bundle/dsseEnvelope/payload")
                    .and_then(Value::as_str)
                    .and_then(|p| {
                        let decoded = ghc_core::text::base64_decode(p).ok()?;
                        let payload: Value = serde_json::from_slice(&decoded).ok()?;
                        let pred_type = payload.get("predicateType").and_then(Value::as_str)?;
                        if pred_type.contains("release") {
                            Some(true)
                        } else {
                            None
                        }
                    })
                    .is_some()
            })
            .collect();

        if filtered.is_empty() {
            return Err(anyhow::anyhow!(
                "no attestations found for release {tag_name} in {}",
                repo.name(),
            ));
        }

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let arr = Value::Array(filtered.iter().map(|v| (*v).clone()).collect());
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

        ios_eprintln!(ios, "Resolved tag {tag_name} to {digest}");
        ios_eprintln!(ios, "Loaded attestation from GitHub API");
        ios_eprintln!(
            ios,
            "{} Release {} verified!",
            cs.success_icon(),
            cs.bold(&tag_name),
        );
        ios_println!(ios, "");

        // Print subjects (assets) from the first attestation
        if let Some(att) = filtered.first() {
            print_verified_subjects(ios, att)?;
        }

        Ok(())
    }
}

/// Print the verified subjects (assets) from an attestation.
fn print_verified_subjects(
    ios: &ghc_core::iostreams::IOStreams,
    attestation: &Value,
) -> Result<()> {
    let payload_b64 = attestation
        .pointer("/bundle/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .unwrap_or("");

    if payload_b64.is_empty() {
        return Ok(());
    }

    let decoded = ghc_core::text::base64_decode(payload_b64)
        .map_err(|e| anyhow::anyhow!("failed to decode attestation payload: {e}"))?;
    let statement: Value =
        serde_json::from_slice(&decoded).context("failed to parse attestation statement")?;

    let subjects = statement
        .get("subject")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // If there are fewer than 2 subjects, there are no assets to display
    if subjects.len() < 2 {
        return Ok(());
    }

    let cs = ios.color_scheme();
    ios_println!(ios, "{}", cs.bold("Assets"));

    let mut tp = TablePrinter::new(ios);

    for subject in &subjects {
        let name = subject.get("name").and_then(Value::as_str).unwrap_or("");
        let digest_map = subject.get("digest").and_then(Value::as_object);

        if !name.is_empty() {
            let digest_str = digest_map
                .and_then(|d| {
                    d.iter()
                        .next()
                        .map(|(k, v)| format!("{k}:{}", v.as_str().unwrap_or("")))
                })
                .unwrap_or_default();

            tp.add_row(vec![name.to_string(), digest_str]);
        }
    }

    let output = tp.render();
    ios_println!(ios, "{output}");
    ios_println!(ios, "");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_rest_get};

    #[tokio::test]
    async fn test_should_verify_release_with_tag() {
        let h = TestHarness::new().await;

        // Mock tag ref
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/git/ref/tags/v1.0.0",
            serde_json::json!({
                "object": { "sha": "abc123" }
            }),
        )
        .await;

        // Mock attestations
        let statement = serde_json::json!({
            "predicateType": "https://github.com/attestations/release/v0.1",
            "subject": [
                { "name": "release-tag", "digest": {"sha1": "abc123"} },
                { "name": "my-binary.tar.gz", "digest": {"sha256": "def456"} }
            ]
        });
        let payload =
            ghc_core::text::base64_encode(serde_json::to_string(&statement).unwrap().as_bytes());

        mock_rest_get(
            &h.server,
            "/repos/owner/repo/attestations/sha1:abc123",
            serde_json::json!({
                "attestations": [{
                    "bundle": {
                        "dsseEnvelope": {
                            "payloadType": "application/vnd.in-toto+json",
                            "payload": payload,
                        }
                    }
                }]
            }),
        )
        .await;

        let args = VerifyArgs {
            tag: Some("v1.0.0".into()),
            repo: Some("owner/repo".into()),
            json: vec![],
            jq: None,
            template: None,
        };
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(err.contains("Resolved tag v1.0.0"));
        assert!(err.contains("verified"));
    }
}
