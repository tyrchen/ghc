//! Agent task commands (`ghc agent-task`).
//!
//! Manage AI agent tasks on GitHub.

use anyhow::{Context, Result};
use clap::Subcommand;
use ghc_core::{ios_eprintln, ios_println};
use serde_json::Value;

/// Manage AI agent tasks.
#[derive(Debug, Subcommand)]
pub enum AgentTaskCommand {
    /// Create an agent task.
    Create(CreateArgs),
    /// List agent tasks.
    List(ListArgs),
    /// View an agent task.
    View(ViewArgs),
}

impl AgentTaskCommand {
    /// Run the agent-task subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        match self {
            Self::Create(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}

/// Create an agent task.
#[derive(Debug, clap::Args)]
pub struct CreateArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: String,

    /// Task description.
    #[arg(short, long)]
    body: String,

    /// Task title.
    #[arg(short, long)]
    title: Option<String>,
}

impl CreateArgs {
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let mut body = serde_json::json!({
            "body": self.body,
        });
        if let Some(ref title) = self.title {
            body["title"] = Value::String(title.clone());
        }

        let path = format!("repos/{}/{}/agent-tasks", repo.owner(), repo.name());
        let result: Value = client
            .rest(reqwest::Method::POST, &path, Some(&body))
            .await
            .context("failed to create agent task")?;

        let id = result.get("id").and_then(Value::as_i64).unwrap_or(0);

        ios_eprintln!(ios, "{} Created agent task #{id}", cs.success_icon());

        Ok(())
    }
}

/// List agent tasks.
#[derive(Debug, clap::Args)]
pub struct ListArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: String,

    /// Maximum number of tasks to list.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let path = format!(
            "repos/{}/{}/agent-tasks?per_page={}",
            repo.owner(),
            repo.name(),
            self.limit
        );
        let tasks: Vec<Value> = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to list agent tasks")?;

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&tasks)?);
            return Ok(());
        }

        if tasks.is_empty() {
            ios_eprintln!(ios, "No agent tasks found");
            return Ok(());
        }

        let mut tp = ghc_core::table::TablePrinter::new(ios);
        for task in &tasks {
            let id = task.get("id").and_then(Value::as_i64).unwrap_or(0);
            let title = task
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("(no title)");
            let status = task
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let created = task.get("created_at").and_then(Value::as_str).unwrap_or("");

            tp.add_row(vec![
                cs.bold(&format!("#{id}")),
                title.to_string(),
                status.to_string(),
                created.to_string(),
            ]);
        }
        ios_println!(ios, "{}", tp.render());

        Ok(())
    }
}

/// View an agent task.
#[derive(Debug, clap::Args)]
pub struct ViewArgs {
    /// Repository (OWNER/REPO).
    #[arg(short = 'R', long)]
    repo: String,

    /// Task ID.
    #[arg(value_name = "ID")]
    id: i64,

    /// Open in web browser.
    #[arg(short, long)]
    web: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ViewArgs {
    async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        if self.web {
            let url = format!(
                "https://{}/{}/{}/agent-tasks/{}",
                repo.host(),
                repo.owner(),
                repo.name(),
                self.id
            );
            factory.browser().open(&url)?;
            return Ok(());
        }

        let path = format!(
            "repos/{}/{}/agent-tasks/{}",
            repo.owner(),
            repo.name(),
            self.id
        );
        let task: Value = client
            .rest(reqwest::Method::GET, &path, None::<&Value>)
            .await
            .context("failed to view agent task")?;

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&task)?);
            return Ok(());
        }

        let title = task
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("(no title)");
        let body_text = task.get("body").and_then(Value::as_str).unwrap_or("");
        let status = task
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        ios_println!(ios, "{}", cs.bold(title));
        ios_println!(ios, "Status: {status}");
        if !body_text.is_empty() {
            ios_println!(ios, "\n{body_text}");
        }

        Ok(())
    }
}
