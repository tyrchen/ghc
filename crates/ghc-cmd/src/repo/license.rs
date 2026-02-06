//! `ghc repo license` sub-commands.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Deserialize;

use ghc_core::ios_println;

use crate::factory::Factory;

/// Explore repository licenses.
#[derive(Debug, Subcommand)]
pub enum LicenseCommand {
    /// List commonly used repository licenses.
    List(ListArgs),
    /// View a specific repository license.
    View(ViewArgs),
}

impl LicenseCommand {
    /// Run the sub-command.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        match self {
            Self::List(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}

// ---------------------------------------------------------------------------
// license list
// ---------------------------------------------------------------------------

/// List commonly used repository licenses.
///
/// For even more licenses, visit <https://choosealicense.com/appendix>.
#[derive(Debug, Args)]
pub struct ListArgs;

#[derive(Debug, Deserialize)]
struct LicenseSummary {
    key: String,
    name: String,
    spdx_id: String,
}

impl ListArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let licenses: Vec<LicenseSummary> = client
            .rest(reqwest::Method::GET, "licenses", None)
            .await
            .context("failed to list licenses")?;

        if licenses.is_empty() {
            bail!("no repository licenses found");
        }

        ios_println!(
            ios,
            "{:<20} {:<15} {}",
            cs.bold("LICENSE KEY"),
            cs.bold("SPDX ID"),
            cs.bold("LICENSE NAME"),
        );

        for lic in &licenses {
            ios_println!(ios, "{:<20} {:<15} {}", lic.key, lic.spdx_id, lic.name);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// license view
// ---------------------------------------------------------------------------

/// View a specific repository license by license key or SPDX ID.
///
/// Run `ghc repo license list` to see available commonly used licenses.
/// For even more licenses, visit <https://choosealicense.com/appendix>.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// License key or SPDX ID (e.g., "mit", "MIT", "agpl-3.0").
    #[arg(value_name = "LICENSE")]
    license: String,

    /// Open https://choosealicense.com/ in the browser.
    #[arg(short, long)]
    web: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LicenseDetail {
    key: String,
    name: String,
    spdx_id: String,
    description: String,
    implementation: String,
    html_url: String,
    body: String,
}

impl ViewArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        if self.web {
            let url = format!(
                "https://choosealicense.com/licenses/{}",
                self.license.to_lowercase()
            );
            if factory.io.is_stdout_tty() {
                ios_println!(factory.io, "Opening {} in your browser.", url);
            }
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!("licenses/{}", self.license);
        let license: Result<LicenseDetail, _> =
            client.rest(reqwest::Method::GET, &path, None).await;

        match license {
            Ok(lic) => {
                if ios.is_stdout_tty() {
                    ios_println!(ios, "");
                    ios_println!(ios, "{}", cs.gray(&lic.description));
                    ios_println!(ios, "");
                    ios_println!(
                        ios,
                        "{}",
                        cs.gray(&format!("To implement: {}", lic.implementation))
                    );
                    ios_println!(ios, "");
                    ios_println!(
                        ios,
                        "{}",
                        cs.gray(&format!("For more information, see: {}", lic.html_url))
                    );
                    ios_println!(ios, "");
                }
                ios_println!(ios, "{}", lic.body);
                Ok(())
            }
            Err(ghc_api::errors::ApiError::Http { status: 404, .. }) => {
                bail!(
                    "'{}' is not a valid license name or SPDX ID.\n\n\
                     Run `ghc repo license list` to see available commonly used licenses. \
                     For even more licenses, visit https://choosealicense.com/appendix",
                    self.license
                );
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    use crate::test_helpers::TestHarness;

    use super::*;

    #[tokio::test]
    async fn test_should_list_licenses() {
        let h = TestHarness::new().await;
        Mock::given(method("GET"))
            .and(path("/licenses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "key": "mit", "name": "MIT License", "spdx_id": "MIT" },
                { "key": "apache-2.0", "name": "Apache License 2.0", "spdx_id": "Apache-2.0" },
            ])))
            .mount(&h.server)
            .await;

        let args = ListArgs;
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "list should succeed: {result:?}");
        let stdout = h.stdout();
        assert!(stdout.contains("mit"));
        assert!(stdout.contains("MIT License"));
        assert!(stdout.contains("apache-2.0"));
    }

    #[tokio::test]
    async fn test_should_view_license() {
        let h = TestHarness::new().await;
        Mock::given(method("GET"))
            .and(path("/licenses/mit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "key": "mit",
                "name": "MIT License",
                "spdx_id": "MIT",
                "description": "A short and simple permissive license.",
                "implementation": "Create a text file (typically named LICENSE or LICENSE.md).",
                "html_url": "https://choosealicense.com/licenses/mit/",
                "body": "MIT License\n\nCopyright (c) [year] [fullname]",
            })))
            .mount(&h.server)
            .await;

        let args = ViewArgs {
            license: "mit".into(),
            web: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "view should succeed: {result:?}");
        let stdout = h.stdout();
        assert!(stdout.contains("MIT License"));
        assert!(stdout.contains("Copyright (c)"));
    }

    #[tokio::test]
    async fn test_should_fail_view_unknown_license() {
        let h = TestHarness::new().await;
        Mock::given(method("GET"))
            .and(path("/licenses/unknown-lic"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "message": "Not Found",
                "documentation_url": "https://docs.github.com"
            })))
            .mount(&h.server)
            .await;

        let args = ViewArgs {
            license: "unknown-lic".into(),
            web: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not a valid license"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn test_should_open_web_for_license_view() {
        let h = TestHarness::new().await;
        let args = ViewArgs {
            license: "mit".into(),
            web: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok());
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("choosealicense.com/licenses/mit"));
    }
}
