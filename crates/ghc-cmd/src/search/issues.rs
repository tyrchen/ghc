//! `ghc search issues` command.

use std::fmt::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::text;
use ghc_core::{ios_eprintln, ios_println};

/// Search for issues across GitHub.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct IssuesArgs {
    /// Search query.
    #[arg(value_name = "QUERY", required = true)]
    query: Vec<String>,

    /// Maximum number of results.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Filter by repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: Vec<String>,

    /// Filter by state.
    #[arg(long, value_parser = ["open", "closed"])]
    state: Option<String>,

    /// Filter by author.
    #[arg(long)]
    author: Option<String>,

    /// Filter by assignee.
    #[arg(long)]
    assignee: Option<String>,

    /// Filter by label.
    #[arg(long)]
    label: Vec<String>,

    /// Filter by language.
    #[arg(long)]
    language: Option<String>,

    /// Include pull requests in results.
    #[arg(long)]
    include_prs: bool,

    /// Filter by GitHub App author.
    #[arg(long)]
    app: Option<String>,

    /// Filter on closed at date.
    #[arg(long)]
    closed: Option<String>,

    /// Filter based on comments by user.
    #[arg(long)]
    commenter: Option<String>,

    /// Filter on number of comments.
    #[arg(long)]
    comments: Option<String>,

    /// Filter based on created at date.
    #[arg(long)]
    created: Option<String>,

    /// Filter on number of reactions and comments.
    #[arg(long)]
    interactions: Option<String>,

    /// Filter based on involvement of user.
    #[arg(long)]
    involves: Option<String>,

    /// Filter on locked conversation status.
    #[arg(long)]
    locked: bool,

    /// Filter based on user mentions.
    #[arg(long)]
    mentions: Option<String>,

    /// Filter by milestone title.
    #[arg(long)]
    milestone: Option<String>,

    /// Filter on missing assignee.
    #[arg(long)]
    no_assignee: bool,

    /// Filter on missing label.
    #[arg(long)]
    no_label: bool,

    /// Filter on missing milestone.
    #[arg(long)]
    no_milestone: bool,

    /// Filter on missing project.
    #[arg(long)]
    no_project: bool,

    /// Filter on project board owner/number.
    #[arg(long)]
    project: Option<String>,

    /// Filter on number of reactions.
    #[arg(long)]
    reactions: Option<String>,

    /// Filter based on team mentions.
    #[arg(long)]
    team_mentions: Option<String>,

    /// Filter on last updated at date.
    #[arg(long)]
    updated: Option<String>,

    /// Filter on repository owner.
    #[arg(long)]
    owner: Vec<String>,

    /// Sort results.
    #[arg(long, value_parser = ["comments", "created", "interactions", "reactions", "updated"])]
    sort: Option<String>,

    /// Sort order.
    #[arg(long, value_parser = ["asc", "desc"], default_value = "desc")]
    order: String,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,

    /// Open results in the browser.
    #[arg(short, long)]
    web: bool,
}

impl IssuesArgs {
    /// Run the search issues command.
    ///
    /// # Errors
    ///
    /// Returns an error if the search fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let mut q = self.query.join(" ");
        if !self.include_prs {
            q.push_str(" type:issue");
        }

        for repo in &self.repo {
            let _ = write!(q, " repo:{repo}");
        }
        if let Some(ref state) = self.state {
            let _ = write!(q, " state:{state}");
        }
        if let Some(ref author) = self.author {
            let _ = write!(q, " author:{author}");
        }
        if let Some(ref app) = self.app {
            let _ = write!(q, " author:app/{app}");
        }
        if let Some(ref assignee) = self.assignee {
            let _ = write!(q, " assignee:{assignee}");
        }
        for label in &self.label {
            let _ = write!(q, " label:{label}");
        }
        if let Some(ref lang) = self.language {
            let _ = write!(q, " language:{lang}");
        }
        if let Some(ref closed) = self.closed {
            let _ = write!(q, " closed:{closed}");
        }
        if let Some(ref commenter) = self.commenter {
            let _ = write!(q, " commenter:{commenter}");
        }
        if let Some(ref comments) = self.comments {
            let _ = write!(q, " comments:{comments}");
        }
        if let Some(ref created) = self.created {
            let _ = write!(q, " created:{created}");
        }
        if let Some(ref interactions) = self.interactions {
            let _ = write!(q, " interactions:{interactions}");
        }
        if let Some(ref involves) = self.involves {
            let _ = write!(q, " involves:{involves}");
        }
        if self.locked {
            q.push_str(" is:locked");
        }
        if let Some(ref mentions) = self.mentions {
            let _ = write!(q, " mentions:{mentions}");
        }
        if let Some(ref milestone) = self.milestone {
            let _ = write!(q, " milestone:{milestone}");
        }
        if self.no_assignee {
            q.push_str(" no:assignee");
        }
        if self.no_label {
            q.push_str(" no:label");
        }
        if self.no_milestone {
            q.push_str(" no:milestone");
        }
        if self.no_project {
            q.push_str(" no:project");
        }
        if let Some(ref project) = self.project {
            let _ = write!(q, " project:{project}");
        }
        if let Some(ref reactions) = self.reactions {
            let _ = write!(q, " reactions:{reactions}");
        }
        if let Some(ref team) = self.team_mentions {
            let _ = write!(q, " team:{team}");
        }
        if let Some(ref updated) = self.updated {
            let _ = write!(q, " updated:{updated}");
        }
        for owner in &self.owner {
            let _ = write!(q, " user:{owner}");
        }

        if self.web {
            let encoded = ghc_core::text::percent_encode(&q);
            let url = format!("https://github.com/search?q={encoded}&type=issues");
            factory.browser().open(&url)?;
            return Ok(());
        }

        let client = factory.api_client("github.com")?;
        let ios = &factory.io;

        let encoded = ghc_core::text::percent_encode(&q);
        let mut path = format!("search/issues?q={encoded}&per_page={}", self.limit.min(100));
        if let Some(ref sort) = self.sort {
            let _ = write!(path, "&sort={sort}&order={}", self.order);
        }

        let result: Value = client
            .rest(reqwest::Method::GET, &path, None)
            .await
            .context("failed to search issues")?;

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let output = ghc_core::json::format_json_output(
                &result,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            return Ok(());
        }

        let items = result
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("unexpected search response format"))?;

        if items.is_empty() {
            ios_eprintln!(ios, "No issues matched your search");
            return Ok(());
        }

        let cs = ios.color_scheme();
        let mut tp = TablePrinter::new(ios);

        for item in items {
            let number = item.get("number").and_then(Value::as_u64).unwrap_or(0);
            let title = item.get("title").and_then(Value::as_str).unwrap_or("");
            let state = item.get("state").and_then(Value::as_str).unwrap_or("");
            let repo_url = item
                .get("repository_url")
                .and_then(Value::as_str)
                .unwrap_or("");
            let repo_name = repo_url.rsplit('/').take(2).collect::<Vec<_>>();
            let repo_display = if repo_name.len() >= 2 {
                format!("{}/{}", repo_name[1], repo_name[0])
            } else {
                String::new()
            };

            let state_display = if state == "open" {
                cs.success("open")
            } else {
                cs.error("closed")
            };

            tp.add_row(vec![
                cs.bold(&repo_display),
                format!("#{number}"),
                text::truncate(title, 60),
                state_display,
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_rest_get};

    fn default_args(query: &str) -> IssuesArgs {
        IssuesArgs {
            query: vec![query.to_string()],
            limit: 30,
            repo: vec![],
            state: None,
            author: None,
            assignee: None,
            label: vec![],
            language: None,
            include_prs: false,
            app: None,
            closed: None,
            commenter: None,
            comments: None,
            created: None,
            interactions: None,
            involves: None,
            locked: false,
            mentions: None,
            milestone: None,
            no_assignee: false,
            no_label: false,
            no_milestone: false,
            no_project: false,
            project: None,
            reactions: None,
            team_mentions: None,
            updated: None,
            owner: vec![],
            sort: None,
            order: "desc".to_string(),
            json: vec![],
            jq: None,
            template: None,
            web: false,
        }
    }

    fn search_issues_response() -> serde_json::Value {
        serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "number": 42,
                    "title": "Found Issue",
                    "state": "open",
                    "repository_url": "https://api.github.com/repos/owner/repo"
                }
            ]
        })
    }

    #[tokio::test]
    async fn test_should_search_issues() {
        let h = TestHarness::new().await;
        mock_rest_get(&h.server, "/search/issues", search_issues_response()).await;

        let args = default_args("bug fix");
        args.run(&h.factory).await.unwrap();

        let out = h.stdout();
        assert!(out.contains("#42"), "should contain issue number");
        assert!(out.contains("Found Issue"), "should contain issue title");
    }

    #[tokio::test]
    async fn test_should_open_browser_in_web_mode() {
        let h = TestHarness::new().await;
        let mut args = default_args("bug fix");
        args.web = true;
        args.run(&h.factory).await.unwrap();

        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert!(
            urls[0].contains("type=issues"),
            "should open search URL with issue type"
        );
    }

    #[tokio::test]
    async fn test_should_show_empty_message_when_no_results() {
        let h = TestHarness::new().await;
        mock_rest_get(
            &h.server,
            "/search/issues",
            serde_json::json!({ "total_count": 0, "items": [] }),
        )
        .await;

        let args = default_args("nonexistent");
        args.run(&h.factory).await.unwrap();

        let err = h.stderr();
        assert!(
            err.contains("No issues matched"),
            "should show empty message"
        );
    }
}
