//! `ghc issue create` command.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Create a new issue.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Issue title.
    #[arg(short, long)]
    title: Option<String>,

    /// Issue body text.
    #[arg(short, long, conflicts_with = "body_file")]
    body: Option<String>,

    /// Read body text from file (use "-" to read from standard input).
    #[arg(short = 'F', long, conflicts_with = "body")]
    body_file: Option<PathBuf>,

    /// Skip prompts and open the text editor to write the title and body.
    #[arg(short, long)]
    editor: bool,

    /// Assignee logins (comma-separated). Use "@me" to self-assign.
    #[arg(short, long, value_delimiter = ',')]
    assignee: Vec<String>,

    /// Label names (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    label: Vec<String>,

    /// Project names to add the issue to (comma-separated).
    #[arg(short, long, value_delimiter = ',')]
    project: Vec<String>,

    /// Milestone name.
    #[arg(short, long)]
    milestone: Option<String>,

    /// Template name to use as starting body text.
    #[arg(short = 'T', long)]
    template: Option<String>,

    /// Open the new issue in the browser.
    #[arg(short, long)]
    web: bool,
}

impl CreateArgs {
    /// Run the issue create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository format is invalid, required fields
    /// are missing, or the API request fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        if self.template.is_some() && (self.body.is_some() || self.body_file.is_some()) {
            anyhow::bail!("`--template` is not supported when using `--body` or `--body-file`");
        }

        if self.web {
            let url = format!(
                "https://{}/{}/{}/issues/new",
                repo.host(),
                repo.owner(),
                repo.name(),
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        // Resolve body from --body-file if provided
        let body_from_file = if let Some(ref body_file) = self.body_file {
            Some(read_body_file(body_file).context("failed to read body file")?)
        } else {
            None
        };

        // Resolve template body if --template is given
        let template_body = if let Some(ref template_name) = self.template {
            let client = factory.api_client(repo.host())?;
            fetch_issue_template_body(&client, &repo, template_name).await?
        } else {
            None
        };

        // Determine title
        let title = if let Some(t) = &self.title {
            t.clone()
        } else if self.editor {
            // In editor mode, title is entered via editor (first line)
            String::new()
        } else {
            let prompter = factory.prompter();
            prompter
                .input("Title", "")
                .context("failed to read title")?
        };

        // Determine body
        let (final_title, final_body) = if self.editor {
            let default_body = body_from_file
                .as_deref()
                .or(template_body.as_deref())
                .unwrap_or("");
            let editor_content = format!("{title}\n{default_body}");
            let prompter = factory.prompter();
            let edited = prompter
                .editor("Issue", &editor_content, true)
                .context("failed to read from editor")?;
            // First line is the title, rest is body
            let mut lines = edited.splitn(2, '\n');
            let t = lines.next().unwrap_or("").trim().to_string();
            let b = lines.next().unwrap_or("").trim().to_string();
            (t, b)
        } else {
            let body = if let Some(b) = &self.body {
                b.clone()
            } else if let Some(b) = body_from_file {
                b
            } else {
                let default_body = template_body.as_deref().unwrap_or("");
                let prompter = factory.prompter();
                prompter
                    .editor("Body", default_body, true)
                    .context("failed to read body")?
            };
            (title, body)
        };

        if final_title.is_empty() {
            anyhow::bail!("title is required");
        }

        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        // Resolve @me in assignees
        let assignees: Vec<String> = self
            .assignee
            .iter()
            .map(|a| {
                if a == "@me" {
                    // The API will accept @me and resolve it server-side
                    a.clone()
                } else {
                    a.clone()
                }
            })
            .collect();

        let path = format!("repos/{}/{}/issues", repo.owner(), repo.name());
        let mut request_body = serde_json::json!({
            "title": final_title,
            "body": final_body,
        });

        if !assignees.is_empty() {
            request_body["assignees"] =
                Value::Array(assignees.iter().map(|a| Value::String(a.clone())).collect());
        }

        if !self.label.is_empty() {
            request_body["labels"] = Value::Array(
                self.label
                    .iter()
                    .map(|l| Value::String(l.clone()))
                    .collect(),
            );
        }

        if let Some(ref milestone) = self.milestone {
            let milestone_number = resolve_milestone(&client, &repo, milestone).await?;
            request_body["milestone"] = Value::Number(serde_json::Number::from(milestone_number));
        }

        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&request_body))
            .await
            .context("failed to create issue")?;

        let number = result.get("number").and_then(Value::as_i64).unwrap_or(0);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Created issue #{} in {}",
            cs.success_icon(),
            number,
            cs.bold(&repo.full_name()),
        );
        ios_println!(ios, "{}", text::display_url(html_url));

        Ok(())
    }
}

/// Read body text from a file path or stdin (`-`).
pub(crate) fn read_body_file(path: &std::path::Path) -> Result<String> {
    if path == std::path::Path::new("-") {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read file: {}", path.display()))
    }
}

/// Fetch a specific issue template's body from the repository.
async fn fetch_issue_template_body(
    client: &ghc_api::client::Client,
    repo: &ghc_core::repo::Repo,
    template_name: &str,
) -> Result<Option<String>> {
    // Try to fetch templates from the repository via the API
    let path = format!(
        "repos/{}/{}/contents/.github/ISSUE_TEMPLATE",
        repo.owner(),
        repo.name(),
    );
    let contents: Result<Vec<Value>, _> = client
        .rest(reqwest::Method::GET, &path, None::<&Value>)
        .await;

    let Ok(files) = contents else {
        return Ok(None);
    };

    // Find the matching template file
    for file in &files {
        let name = file.get("name").and_then(Value::as_str).unwrap_or("");
        let name_without_ext = name
            .strip_suffix(".md")
            .or_else(|| name.strip_suffix(".yml"))
            .or_else(|| name.strip_suffix(".yaml"))
            .unwrap_or(name);

        if name_without_ext.eq_ignore_ascii_case(template_name)
            || name.eq_ignore_ascii_case(template_name)
        {
            // Fetch the file content
            if let Some(download_url) = file.get("download_url").and_then(Value::as_str) {
                let content = client
                    .rest_text(reqwest::Method::GET, download_url, None)
                    .await
                    .context("failed to fetch template content")?;
                // Strip YAML front matter if present
                let body = strip_front_matter(&content);
                return Ok(Some(body));
            }
        }
    }

    Ok(None)
}

/// Strip YAML front matter (between --- delimiters) from template content.
fn strip_front_matter(content: &str) -> String {
    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---")
        && let Some(end) = rest.find("---")
    {
        return rest[end + 3..].trim_start().to_string();
    }
    content.to_string()
}

/// Resolve a milestone name to its number via the REST API.
async fn resolve_milestone(
    client: &ghc_api::client::Client,
    repo: &ghc_core::repo::Repo,
    milestone_name: &str,
) -> Result<i64> {
    let path = format!(
        "repos/{}/{}/milestones?state=open&per_page=100",
        repo.owner(),
        repo.name(),
    );

    let milestones: Vec<Value> = client
        .rest(reqwest::Method::GET, &path, None)
        .await
        .context("failed to fetch milestones")?;

    for ms in &milestones {
        let title = ms.get("title").and_then(Value::as_str).unwrap_or("");
        if title.eq_ignore_ascii_case(milestone_name) {
            return ms
                .get("number")
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow::anyhow!("milestone missing number field"));
        }
    }

    anyhow::bail!(
        "milestone {milestone_name:?} not found; available milestones: {}",
        milestones
            .iter()
            .filter_map(|ms| ms.get("title").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_post};

    fn default_args(repo: &str) -> CreateArgs {
        CreateArgs {
            repo: repo.to_string(),
            title: Some("Test Issue".to_string()),
            body: Some("Test body".to_string()),
            body_file: None,
            editor: false,
            assignee: vec![],
            label: vec![],
            project: vec![],
            milestone: None,
            template: None,
            web: false,
        }
    }

    #[tokio::test]
    async fn test_should_create_issue() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues",
            201,
            serde_json::json!({
                "number": 42,
                "html_url": "https://github.com/owner/repo/issues/42"
            }),
        )
        .await;

        let args = default_args("owner/repo");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(
            out.contains("github.com/owner/repo/issues/42"),
            "should contain issue URL"
        );
        let err = h.stderr();
        assert!(
            err.contains("Created issue #42"),
            "should show created message"
        );
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("owner/repo");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("/issues/new"), "should open new issue URL");
    }

    #[tokio::test]
    async fn test_should_fail_with_empty_title() {
        let h = TestHarness::new().await;
        let mut args = default_args("owner/repo");
        args.title = Some(String::new());

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("title is required")
        );
    }

    #[tokio::test]
    async fn test_should_fail_when_template_with_body() {
        let h = TestHarness::new().await;
        let mut args = default_args("owner/repo");
        args.template = Some("bug-report".to_string());

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--template"),);
    }

    #[tokio::test]
    async fn test_should_create_issue_from_body_file() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/repos/owner/repo/issues",
            201,
            serde_json::json!({
                "number": 43,
                "html_url": "https://github.com/owner/repo/issues/43"
            }),
        )
        .await;

        // Create a temp file with body content
        let tmp_dir = std::env::temp_dir();
        let body_path = tmp_dir.join("test_issue_body.txt");
        std::fs::write(&body_path, "Body from file").unwrap();

        let mut args = default_args("owner/repo");
        args.body = None;
        args.body_file = Some(body_path.clone());
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("Created issue #43"),
            "should show created message"
        );

        // Clean up
        let _ = std::fs::remove_file(&body_path);
    }

    #[test]
    fn test_should_strip_front_matter() {
        let content = "---\nname: Bug Report\nabout: Report a bug\n---\n\nActual body here";
        let result = strip_front_matter(content);
        assert_eq!(result, "Actual body here");
    }

    #[test]
    fn test_should_not_strip_content_without_front_matter() {
        let content = "Just regular content";
        let result = strip_front_matter(content);
        assert_eq!(result, "Just regular content");
    }
}
