//! `ghc release verify-asset` command.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

/// Verify that a given asset originated from a release.
///
/// Checks that the asset file matches a valid attestation for the specified
/// release (or the latest release, if no tag is given). Validates the asset's
/// digest against the subjects in the attestation.
#[derive(Debug, Args)]
pub struct VerifyAssetArgs {
    /// The release tag. If omitted, uses the latest release.
    #[arg(value_name = "TAG")]
    tag: Option<String>,

    /// Path to the asset file to verify.
    #[arg(value_name = "FILE")]
    file: String,

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

impl VerifyAssetArgs {
    /// Run the release verify-asset command.
    ///
    /// # Errors
    ///
    /// Returns an error if the asset cannot be verified.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo_str = self
            .repo
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("repository argument required (use -R OWNER/REPO)"))?;
        let repo = Repo::from_full_name(repo_str).context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let file_path = std::path::PathBuf::from(&self.file);
        let file_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.clone());
        let file_name = file_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(&self.file);

        if !Path::new(&self.file).exists() {
            return Err(anyhow::anyhow!("asset file not found: {}", self.file));
        }

        let file_digest = compute_sha256(&self.file).await?;
        let file_digest_with_alg = format!("sha256:{file_digest}");

        let tag_name = resolve_tag(&client, &repo, self.tag.as_deref()).await?;
        let (release_digest, release_attestations) =
            fetch_release_attestations(&client, &repo, &tag_name).await?;

        let matching: Vec<&Value> = release_attestations
            .iter()
            .filter(|att| attestation_contains_digest(att, &file_digest))
            .collect();

        if matching.is_empty() {
            return Err(anyhow::anyhow!(
                "attestation for {tag_name} does not contain subject {file_digest_with_alg}",
            ));
        }

        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let arr = Value::Array(matching.iter().map(|v| (*v).clone()).collect());
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

        ios_eprintln!(
            ios,
            "Calculated digest for {file_name}: {file_digest_with_alg}"
        );
        ios_eprintln!(ios, "Resolved tag {tag_name} to {release_digest}");
        ios_eprintln!(ios, "Loaded attestation from GitHub API");
        ios_println!(ios, "");
        ios_eprintln!(
            ios,
            "{} Verification succeeded! {file_name} is present in release {tag_name}",
            cs.success_icon(),
        );

        Ok(())
    }
}

/// Resolve a tag name, fetching the latest release if none is specified.
async fn resolve_tag(
    client: &ghc_api::client::Client,
    repo: &Repo,
    tag: Option<&str>,
) -> Result<String> {
    if let Some(tag) = tag {
        return Ok(tag.to_string());
    }
    let path = format!("repos/{}/{}/releases/latest", repo.owner(), repo.name());
    let release: Value = client
        .rest(reqwest::Method::GET, &path, None::<&Value>)
        .await
        .context("failed to fetch latest release")?;
    release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("no tag_name in latest release"))
        .map(String::from)
}

/// Fetch release attestations for a given tag, returning the digest and filtered attestations.
async fn fetch_release_attestations(
    client: &ghc_api::client::Client,
    repo: &Repo,
    tag_name: &str,
) -> Result<(String, Vec<Value>)> {
    let ref_path = format!(
        "repos/{}/{}/git/ref/tags/{tag_name}",
        repo.owner(),
        repo.name(),
    );
    let ref_data: Value = client
        .rest(reqwest::Method::GET, &ref_path, None::<&Value>)
        .await
        .context("failed to fetch tag ref")?;

    let sha = ref_data
        .pointer("/object/sha")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("failed to resolve tag {tag_name} to a SHA"))?;

    let release_digest = format!("sha1:{sha}");

    let att_path = format!(
        "repos/{}/{}/attestations/{release_digest}?per_page=100",
        repo.owner(),
        repo.name(),
    );
    let att_result: Value = client
        .rest(reqwest::Method::GET, &att_path, None::<&Value>)
        .await
        .with_context(|| format!("no attestations for tag {tag_name} ({release_digest})"))?;

    let attestations = att_result
        .get("attestations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let release_attestations: Vec<Value> = attestations
        .into_iter()
        .filter(is_release_attestation)
        .collect();

    if release_attestations.is_empty() {
        return Err(anyhow::anyhow!(
            "no attestations found for release {tag_name} in {}/{}",
            repo.owner(),
            repo.name(),
        ));
    }

    Ok((release_digest, release_attestations))
}

/// Check if an attestation has a release predicate type.
fn is_release_attestation(att: &Value) -> bool {
    att.pointer("/bundle/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .and_then(|p| {
            let decoded = ghc_core::text::base64_decode(p).ok()?;
            let payload: Value = serde_json::from_slice(&decoded).ok()?;
            let pred_type = payload.get("predicateType").and_then(Value::as_str)?;
            pred_type.contains("release").then_some(true)
        })
        .is_some()
}

/// Check if an attestation contains a subject with the specified SHA256 digest.
fn attestation_contains_digest(att: &Value, file_digest: &str) -> bool {
    att.pointer("/bundle/dsseEnvelope/payload")
        .and_then(Value::as_str)
        .and_then(|p| {
            let decoded = ghc_core::text::base64_decode(p).ok()?;
            let payload: Value = serde_json::from_slice(&decoded).ok()?;
            let subjects = payload.get("subject").and_then(Value::as_array)?;
            for subject in subjects {
                if let Some(sha256) = subject
                    .get("digest")
                    .and_then(Value::as_object)
                    .and_then(|d| d.get("sha256"))
                    .and_then(Value::as_str)
                    && sha256 == file_digest
                {
                    return Some(true);
                }
            }
            None
        })
        .is_some()
}

/// Compute the SHA256 hex digest of a file.
async fn compute_sha256(path: &str) -> Result<String> {
    let output = tokio::process::Command::new("shasum")
        .args(["-a", "256", path])
        .output()
        .await
        .with_context(|| format!("failed to run shasum on asset file: {path}"))?;

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

#[cfg(test)]
mod tests {
    #[test]
    fn test_should_parse_args() {
        // Basic construction test
        let args = super::VerifyAssetArgs {
            tag: Some("v1.0.0".into()),
            file: "my-binary.tar.gz".into(),
            repo: Some("owner/repo".into()),
            json: vec![],
            jq: None,
            template: None,
        };
        assert_eq!(args.tag.as_deref(), Some("v1.0.0"));
        assert_eq!(args.file, "my-binary.tar.gz");
    }
}
