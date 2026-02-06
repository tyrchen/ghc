//! Run commands (`ghc run`).
//!
//! Manage GitHub Actions workflow runs.

pub mod cancel;
pub mod delete;
pub mod download;
pub mod list;
pub mod rerun;
pub mod view;
pub mod watch;

use clap::Subcommand;
use serde_json::Value;

/// Manage workflow runs.
#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// Cancel a workflow run.
    Cancel(cancel::CancelArgs),
    /// Delete a workflow run.
    Delete(delete::DeleteArgs),
    /// Download run artifacts.
    Download(download::DownloadArgs),
    /// List recent workflow runs.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Rerun a workflow run.
    Rerun(rerun::RerunArgs),
    /// View a workflow run.
    View(view::ViewArgs),
    /// Watch a run until it completes.
    Watch(watch::WatchArgs),
}

/// Normalize REST API run field names to match gh CLI conventions.
///
/// Maps `run_number` -> `number`, `id` -> `databaseId`,
/// `html_url` -> `url` (replacing the API URL with the web URL).
pub(crate) fn normalize_run_fields(run: &mut Value) {
    if let Some(obj) = run.as_object_mut() {
        // Map run_number -> number
        if let Some(val) = obj.get("run_number").cloned() {
            obj.insert("number".to_string(), val);
        }
        // Map id -> databaseId
        if let Some(val) = obj.get("id").cloned() {
            obj.insert("databaseId".to_string(), val);
        }
        // Map html_url -> url (web URL replaces API URL)
        if let Some(val) = obj.get("html_url").cloned() {
            obj.insert("url".to_string(), val);
        }
    }
}

/// Normalize run fields for each element in a JSON array.
pub(crate) fn normalize_run_fields_array(value: &mut Value) {
    if let Some(arr) = value.as_array_mut() {
        for item in arr {
            normalize_run_fields(item);
        }
    }
}

impl RunCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Cancel(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Download(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Rerun(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
            Self::Watch(args) => args.run(factory).await,
        }
    }
}
