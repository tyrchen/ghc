//! `ghc repo autolink` sub-commands.
//!
//! Autolinks link issues, pull requests, commit messages, and release
//! descriptions to external third-party services.
//!
//! Autolinks require `admin` role to view or manage.

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

use crate::factory::Factory;

/// Manage autolink references in a repository.
#[derive(Debug, Subcommand)]
pub enum AutolinkCommand {
    /// Create a new autolink reference.
    Create(CreateArgs),
    /// Delete an autolink reference.
    Delete(DeleteArgs),
    /// List autolink references in a repository.
    List(ListArgs),
    /// View an autolink reference.
    View(ViewArgs),
}

impl AutolinkCommand {
    /// Run the sub-command.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        match self {
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}

/// An autolink reference.
#[derive(Debug, Serialize, Deserialize)]
struct Autolink {
    id: i64,
    key_prefix: String,
    url_template: String,
    is_alphanumeric: bool,
}

// ---------------------------------------------------------------------------
// autolink create
// ---------------------------------------------------------------------------

/// Create a new autolink reference for a repository.
///
/// The key prefix specifies the prefix that generates a link when appended by
/// certain characters. The URL template must contain `<num>` for the reference
/// number.
///
/// By default autolinks are alphanumeric. Use `--numeric` for numeric only.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The key prefix for the autolink reference.
    #[arg(value_name = "KEY-PREFIX")]
    key_prefix: String,

    /// The URL template with `<num>` placeholder.
    #[arg(value_name = "URL-TEMPLATE")]
    url_template: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Mark autolink as numeric only (default is alphanumeric).
    #[arg(short, long)]
    numeric: bool,
}

impl CreateArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let body = serde_json::json!({
            "key_prefix": self.key_prefix,
            "url_template": self.url_template,
            "is_alphanumeric": !self.numeric,
        });

        let api_path = format!("repos/{}/{}/autolinks", repo.owner(), repo.name());
        let result: Result<Autolink, _> = client
            .rest(reqwest::Method::POST, &api_path, Some(&body))
            .await;

        match result {
            Ok(autolink) => {
                ios_println!(
                    ios,
                    "{} Created repository autolink {} on {}",
                    cs.success_icon(),
                    cs.cyan(&autolink.id.to_string()),
                    cs.bold(&repo.full_name()),
                );
                Ok(())
            }
            Err(ghc_api::errors::ApiError::Http { status: 404, .. }) => {
                bail!("must have admin rights to repository");
            }
            Err(e) => Err(anyhow::anyhow!("error creating autolink: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// autolink delete
// ---------------------------------------------------------------------------

/// Delete an autolink reference from a repository.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// ID of the autolink to delete.
    #[arg(value_name = "ID")]
    id: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Skip confirmation prompt.
    #[arg(long)]
    yes: bool,
}

impl DeleteArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Fetch autolink details first (for confirmation display)
        let view_path = format!(
            "repos/{}/{}/autolinks/{}",
            repo.owner(),
            repo.name(),
            self.id,
        );
        let autolink: Autolink = client
            .rest(reqwest::Method::GET, &view_path, None)
            .await
            .with_context(|| format!("error fetching autolink {}", self.id))?;

        if !self.yes {
            if !ios.can_prompt() {
                bail!("--yes required when not running interactively");
            }
            ios_println!(
                ios,
                "Autolink {} has key prefix {}.",
                cs.cyan(&self.id),
                autolink.key_prefix,
            );
            let answer = factory.prompter().input(
                &format!("Type {} to confirm deletion:", autolink.key_prefix),
                "",
            )?;
            if answer != autolink.key_prefix {
                bail!("confirmation did not match key prefix");
            }
        }

        let delete_path = format!(
            "repos/{}/{}/autolinks/{}",
            repo.owner(),
            repo.name(),
            self.id,
        );
        let result = client
            .rest_text(reqwest::Method::DELETE, &delete_path, None)
            .await;

        match result {
            Ok(_) => {
                if ios.is_stdout_tty() {
                    ios_eprintln!(
                        ios,
                        "{} Autolink {} deleted from {}",
                        cs.success_icon(),
                        cs.cyan(&self.id),
                        cs.bold(&repo.full_name()),
                    );
                }
                Ok(())
            }
            Err(ghc_api::errors::ApiError::Http { status: 404, .. }) => {
                bail!(
                    "error deleting autolink: HTTP 404: Perhaps you are missing admin \
                     rights to the repository?"
                );
            }
            Err(e) => Err(e.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// autolink list
// ---------------------------------------------------------------------------

/// List autolink references for a GitHub repository.
///
/// Information about autolinks is only available to repository administrators.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Open autolink settings in the web browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if self.web {
            let url = format!(
                "https://{}/{}/{}/settings/key_links",
                repo.host(),
                repo.owner(),
                repo.name(),
            );
            if ios.is_stdout_tty() {
                ios_eprintln!(ios, "Opening {} in your browser.", url);
            }
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client(repo.host())?;
        let api_path = format!("repos/{}/{}/autolinks", repo.owner(), repo.name());

        let result: Result<Vec<Autolink>, _> =
            client.rest(reqwest::Method::GET, &api_path, None).await;

        let autolinks = match result {
            Ok(a) => a,
            Err(ghc_api::errors::ApiError::Http { status: 404, .. }) => {
                bail!(
                    "error getting autolinks: HTTP 404: Perhaps you are missing admin \
                     rights to the repository?"
                );
            }
            Err(e) => return Err(e.into()),
        };

        if autolinks.is_empty() {
            bail!("no autolinks found in {}", cs.bold(&repo.full_name()));
        }

        // JSON output
        if !self.json.is_empty() {
            let json_val: Value = serde_json::to_value(&autolinks)?;
            ios_println!(ios, "{}", serde_json::to_string_pretty(&json_val)?);
            return Ok(());
        }

        // Header
        if ios.is_stdout_tty() {
            let count_label = if autolinks.len() == 1 {
                "1 autolink reference".to_string()
            } else {
                format!("{} autolink references", autolinks.len())
            };
            ios_println!(ios, "");
            ios_println!(
                ios,
                "Showing {} in {}",
                count_label,
                cs.bold(&repo.full_name()),
            );
            ios_println!(ios, "");
        }

        // Table
        ios_println!(
            ios,
            "{:<8} {:<20} {:<50} {}",
            cs.bold("ID"),
            cs.bold("KEY PREFIX"),
            cs.bold("URL TEMPLATE"),
            cs.bold("ALPHANUMERIC"),
        );

        for al in &autolinks {
            ios_println!(
                ios,
                "{:<8} {:<20} {:<50} {}",
                cs.cyan(&al.id.to_string()),
                al.key_prefix,
                al.url_template,
                al.is_alphanumeric,
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// autolink view
// ---------------------------------------------------------------------------

/// View an autolink reference for a repository.
#[derive(Debug, Args)]
pub struct ViewArgs {
    /// ID of the autolink to view.
    #[arg(value_name = "ID")]
    id: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let api_path = format!(
            "repos/{}/{}/autolinks/{}",
            repo.owner(),
            repo.name(),
            self.id,
        );

        let result: Result<Autolink, _> = client.rest(reqwest::Method::GET, &api_path, None).await;

        let autolink = match result {
            Ok(a) => a,
            Err(ghc_api::errors::ApiError::Http { status: 404, .. }) => {
                bail!("HTTP 404: Perhaps you are missing admin rights to the repository?");
            }
            Err(e) => return Err(anyhow::anyhow!("error viewing autolink: {e}")),
        };

        // JSON output
        if !self.json.is_empty() {
            let json_val: Value = serde_json::to_value(&autolink)?;
            ios_println!(ios, "{}", serde_json::to_string_pretty(&json_val)?);
            return Ok(());
        }

        ios_println!(ios, "Autolink in {}", cs.bold(&repo.full_name()));
        ios_println!(ios, "");
        ios_println!(
            ios,
            "{}  {}",
            cs.bold("ID:"),
            cs.cyan(&autolink.id.to_string()),
        );
        ios_println!(ios, "{}  {}", cs.bold("Key Prefix:"), autolink.key_prefix);
        ios_println!(
            ios,
            "{}  {}",
            cs.bold("URL Template:"),
            autolink.url_template,
        );
        ios_println!(
            ios,
            "{}  {}",
            cs.bold("Alphanumeric:"),
            autolink.is_alphanumeric,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::test_helpers::{TestHarness, mock_rest_delete, mock_rest_get, mock_rest_post};

    use super::*;

    #[tokio::test]
    async fn test_should_list_autolinks() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/autolinks",
            json!([
                {
                    "id": 1,
                    "key_prefix": "TICKET-",
                    "url_template": "https://example.com/<num>",
                    "is_alphanumeric": true,
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            web: false,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("TICKET-"));
        assert!(stdout.contains("https://example.com/<num>"));
    }

    #[tokio::test]
    async fn test_should_error_when_no_autolinks() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/repos/owner/repo/autolinks", json!([])).await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            web: false,
            json: vec![],
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no autolinks"));
    }

    #[tokio::test]
    async fn test_should_create_autolink() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/autolinks",
            201,
            json!({
                "id": 42,
                "key_prefix": "JIRA-",
                "url_template": "https://jira.example.com/browse/<num>",
                "is_alphanumeric": true,
            }),
        )
        .await;

        let args = CreateArgs {
            key_prefix: "JIRA-".into(),
            url_template: "https://jira.example.com/browse/<num>".into(),
            repo: "owner/repo".into(),
            numeric: false,
        };
        args.run(&h.factory).await.unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("Created repository autolink"));
        assert!(stdout.contains("42"));
    }

    #[tokio::test]
    async fn test_should_create_numeric_autolink() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/autolinks",
            201,
            json!({
                "id": 43,
                "key_prefix": "STORY-",
                "url_template": "https://example.com/STORY?id=<num>",
                "is_alphanumeric": false,
            }),
        )
        .await;

        let args = CreateArgs {
            key_prefix: "STORY-".into(),
            url_template: "https://example.com/STORY?id=<num>".into(),
            repo: "owner/repo".into(),
            numeric: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "create numeric should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_should_view_autolink() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/autolinks/1",
            json!({
                "id": 1,
                "key_prefix": "TICKET-",
                "url_template": "https://example.com/<num>",
                "is_alphanumeric": true,
            }),
        )
        .await;

        let args = ViewArgs {
            id: "1".into(),
            repo: "owner/repo".into(),
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();
        let stdout = h.stdout();
        assert!(stdout.contains("TICKET-"));
        assert!(stdout.contains("https://example.com/<num>"));
    }

    #[tokio::test]
    async fn test_should_delete_autolink_with_yes() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/autolinks/42",
            json!({
                "id": 42,
                "key_prefix": "TICKET-",
                "url_template": "https://example.com/<num>",
                "is_alphanumeric": true,
            }),
        )
        .await;
        mock_rest_delete(&h.server, "/repos/owner/repo/autolinks/42", 204).await;

        let args = DeleteArgs {
            id: "42".into(),
            repo: "owner/repo".into(),
            yes: true,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "delete should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_should_open_web_for_autolink_list() {
        let h = TestHarness::new().await;
        let args = ListArgs {
            repo: "owner/repo".into(),
            web: true,
            json: vec![],
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok());
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("settings/key_links"));
    }
}
