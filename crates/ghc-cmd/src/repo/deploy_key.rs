//! `ghc repo deploy-key` sub-commands.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::Value;

use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};

use crate::factory::Factory;

/// Manage deploy keys in a repository.
#[derive(Debug, Subcommand)]
pub enum DeployKeyCommand {
    /// Add a deploy key to a GitHub repository.
    Add(AddArgs),
    /// Delete a deploy key from a GitHub repository.
    Delete(DeleteArgs),
    /// List deploy keys in a GitHub repository.
    List(ListArgs),
}

impl DeployKeyCommand {
    /// Run the sub-command.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        match self {
            Self::Add(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
        }
    }
}

// ---------------------------------------------------------------------------
// deploy-key add
// ---------------------------------------------------------------------------

/// Add a deploy key to a GitHub repository.
///
/// Note that any key added by ghc will be associated with the current
/// authentication token. If you de-authorize the token, any deploy keys
/// added with it will be removed as well.
#[derive(Debug, Args)]
pub struct AddArgs {
    /// Path to the SSH public key file (use "-" to read from stdin).
    #[arg(value_name = "KEY-FILE")]
    key_file: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Title of the new key.
    #[arg(short, long)]
    title: Option<String>,

    /// Allow write access for the key.
    #[arg(short = 'w', long)]
    allow_write: bool,
}

impl AddArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let key_content = if self.key_file == "-" {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .context("failed to read key from stdin")?;
            buf
        } else {
            let path = Path::new(&self.key_file);
            std::fs::read_to_string(path)
                .with_context(|| format!("failed to read key file: {}", path.display()))?
        };

        let title = self.title.clone().unwrap_or_default();

        let body = serde_json::json!({
            "title": title,
            "key": key_content.trim(),
            "read_only": !self.allow_write,
        });

        let path = format!("repos/{}/{}/keys", repo.owner(), repo.name());
        let _: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to add deploy key")?;

        if ios.is_stdout_tty() {
            ios_eprintln!(
                ios,
                "{} Deploy key added to {}",
                cs.success_icon(),
                cs.bold(&repo.full_name()),
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// deploy-key delete
// ---------------------------------------------------------------------------

/// Delete a deploy key from a GitHub repository.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// ID of the deploy key to delete.
    #[arg(value_name = "KEY-ID")]
    key_id: String,

    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,
}

impl DeleteArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/keys/{}",
            repo.owner(),
            repo.name(),
            self.key_id
        );
        client
            .rest_text(reqwest::Method::DELETE, &path, None)
            .await
            .context("failed to delete deploy key")?;

        if ios.is_stdout_tty() {
            ios_eprintln!(
                ios,
                "{} Deploy key deleted from {}",
                cs.error_icon(),
                cs.bold(&repo.full_name()),
            );
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// deploy-key list
// ---------------------------------------------------------------------------

/// List deploy keys in a GitHub repository.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long, value_name = "REPOSITORY")]
    repo: String,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct DeployKey {
    id: i64,
    key: String,
    title: String,
    created_at: String,
    read_only: bool,
}

impl ListArgs {
    async fn run(&self, factory: &Factory) -> Result<()> {
        let repo =
            Repo::from_full_name(&self.repo).context("invalid repository format (OWNER/REPO)")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!("repos/{}/{}/keys?per_page=100", repo.owner(), repo.name());
        let keys: Vec<DeployKey> = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to list deploy keys")?;

        if keys.is_empty() {
            bail!("no deploy keys found in {}", repo.full_name());
        }

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let json_val: Value = serde_json::to_value(&keys)?;
            let output = ghc_core::json::format_json_output(
                &json_val,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        // Table output
        ios_println!(
            ios,
            "{:<10} {:<30} {:<12} {:<40} {}",
            cs.bold("ID"),
            cs.bold("TITLE"),
            cs.bold("TYPE"),
            cs.bold("KEY"),
            cs.bold("CREATED AT"),
        );

        for key in &keys {
            let key_type = if key.read_only {
                "read-only"
            } else {
                "read-write"
            };
            let truncated_key = truncate_middle(&key.key, 40);
            let created = chrono::DateTime::parse_from_rfc3339(&key.created_at).map_or_else(
                |_| key.created_at.clone(),
                |dt| {
                    let duration = chrono::Utc::now().signed_duration_since(dt);
                    ghc_core::text::fuzzy_ago(duration)
                },
            );
            ios_println!(
                ios,
                "{:<10} {:<30} {:<12} {:<40} {}",
                key.id,
                key.title,
                key_type,
                truncated_key,
                cs.gray(&created),
            );
        }

        Ok(())
    }
}

/// Truncate a string in the middle, replacing the middle with "...".
fn truncate_middle(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        return s.to_string();
    }
    let ellipsis = "...";
    if max_width < ellipsis.len() + 2 {
        return s[..max_width].to_string();
    }
    let half_width = (max_width - ellipsis.len()) / 2;
    let remainder = (max_width - ellipsis.len()) % 2;
    format!(
        "{}{}{}",
        &s[..half_width + remainder],
        ellipsis,
        &s[s.len() - half_width..]
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::test_helpers::{TestHarness, mock_rest_get, mock_rest_post};

    use super::*;

    #[test]
    fn test_should_truncate_middle_short_string() {
        assert_eq!(truncate_middle("hello", 10), "hello");
    }

    #[test]
    fn test_should_truncate_middle_long_string() {
        let s = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI";
        let result = truncate_middle(s, 20);
        assert!(result.len() <= 20);
        assert!(result.contains("..."));
    }

    #[tokio::test]
    async fn test_should_add_deploy_key() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/keys",
            201,
            json!({ "id": 1, "title": "test", "key": "ssh-rsa AAA", "read_only": true }),
        )
        .await;

        let tmp = std::env::temp_dir().join("test_deploy_key.pub");
        std::fs::write(&tmp, "ssh-rsa AAAA test-key").unwrap();

        let args = AddArgs {
            key_file: tmp.display().to_string(),
            repo: "owner/repo".into(),
            title: Some("test".into()),
            allow_write: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "add should succeed: {result:?}");
        std::fs::remove_file(tmp).ok();
    }

    #[tokio::test]
    async fn test_should_list_deploy_keys() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/repos/owner/repo/keys",
            json!([
                {
                    "id": 1,
                    "key": "ssh-rsa AAAA",
                    "title": "CI key",
                    "created_at": "2024-01-15T10:00:00Z",
                    "read_only": true,
                },
                {
                    "id": 2,
                    "key": "ssh-ed25519 BBBB",
                    "title": "Deploy key",
                    "created_at": "2024-01-16T10:00:00Z",
                    "read_only": false,
                }
            ]),
        )
        .await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_ok(), "list should succeed: {result:?}");
        let stdout = h.stdout();
        assert!(stdout.contains("CI key"));
        assert!(stdout.contains("Deploy key"));
        assert!(stdout.contains("read-only"));
        assert!(stdout.contains("read-write"));
    }

    #[tokio::test]
    async fn test_should_fail_list_empty() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/repos/owner/repo/keys", json!([])).await;

        let args = ListArgs {
            repo: "owner/repo".into(),
            json: vec![],
            jq: None,
            template: None,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no deploy keys"));
    }
}
