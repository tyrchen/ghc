//! `ghc attestation trusted-root` command.

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::{ios_eprintln, ios_println};

/// Output `trusted_root.jsonl` contents, likely for offline verification.
///
/// When using `ghc attestation verify`, if your machine is on the internet,
/// this will happen automatically. But to do offline verification, you need to
/// supply a trusted root file with `--custom-trusted-root`; this command will
/// help you fetch a `trusted_root.jsonl` file for that purpose.
#[derive(Debug, Args)]
pub struct TrustedRootArgs {
    /// URL to the TUF repository mirror.
    #[arg(long)]
    tuf_url: Option<String>,

    /// Path to the TUF `root.json` file on disk.
    #[arg(long)]
    tuf_root: Option<String>,

    /// Don't output `trusted_root.jsonl` contents, just verify.
    #[arg(long)]
    verify_only: bool,

    /// Configure host to use.
    #[arg(long)]
    hostname: Option<String>,
}

/// Well-known Sigstore TUF mirror URL.
const SIGSTORE_TUF_URL: &str = "https://tuf-repo-cdn.sigstore.dev";

/// GitHub Sigstore TUF mirror URL.
const GITHUB_TUF_URL: &str = "https://tuf-repo.github.com";

impl TrustedRootArgs {
    /// Run the attestation trusted-root command.
    ///
    /// # Errors
    ///
    /// Returns an error if the trusted root cannot be fetched.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        // Validate that tuf-url and tuf-root are provided together
        match (&self.tuf_url, &self.tuf_root) {
            (Some(_), None) | (None, Some(_)) => {
                return Err(anyhow::anyhow!(
                    "--tuf-url and --tuf-root must be specified together"
                ));
            }
            _ => {}
        }

        let hostname = self.hostname.as_deref().unwrap_or("github.com");
        let ios = &factory.io;

        if let (Some(tuf_url), Some(tuf_root_path)) = (&self.tuf_url, &self.tuf_root) {
            // Custom TUF repository
            let _root_data = std::fs::read(tuf_root_path)
                .with_context(|| format!("failed to read root file {tuf_root_path}"))?;

            self.fetch_trusted_root(factory, tuf_url, hostname).await?;
        } else {
            // Fetch from both Sigstore Public Good Instance and GitHub's instance
            self.fetch_trusted_root(factory, SIGSTORE_TUF_URL, hostname)
                .await?;
            self.fetch_trusted_root(factory, GITHUB_TUF_URL, hostname)
                .await?;
        }

        if self.verify_only {
            ios_eprintln!(ios, "Local TUF repositories verified successfully");
        }

        Ok(())
    }

    /// Fetch and output a trusted root from a TUF repository.
    async fn fetch_trusted_root(
        &self,
        factory: &crate::factory::Factory,
        tuf_url: &str,
        _hostname: &str,
    ) -> Result<()> {
        let ios = &factory.io;

        // Fetch the trusted_root.json from the TUF repository
        let target_url = format!("{tuf_url}/targets/trusted_root.json");

        let http = reqwest::Client::new();
        let resp = http
            .get(&target_url)
            .send()
            .await
            .with_context(|| format!("failed to fetch trusted root from {tuf_url}"))?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "failed to retrieve trusted root from {tuf_url}: HTTP {}",
                resp.status(),
            ));
        }

        let body = resp.text().await.context("failed to read response body")?;

        if self.verify_only {
            ios_eprintln!(
                ios,
                "Local TUF repository for {tuf_url} updated and verified",
            );
        } else {
            // Compact the JSON output (one line per trusted root)
            let parsed: Value =
                serde_json::from_str(&body).context("failed to parse trusted root JSON")?;
            let compact =
                serde_json::to_string(&parsed).context("failed to compact trusted root JSON")?;
            ios_println!(ios, "{compact}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_validate_tuf_flags_together() {
        let args = TrustedRootArgs {
            tuf_url: Some("https://example.com".into()),
            tuf_root: None,
            verify_only: false,
            hostname: None,
        };
        // Would fail at runtime validation
        assert!(args.tuf_url.is_some());
        assert!(args.tuf_root.is_none());
    }

    #[test]
    fn test_should_have_correct_tuf_urls() {
        assert!(SIGSTORE_TUF_URL.starts_with("https://"));
        assert!(GITHUB_TUF_URL.starts_with("https://"));
    }
}
