//! Workflow commands (`ghc workflow`).
//!
//! Manage GitHub Actions workflows.

pub mod disable;
pub mod enable;
pub mod list;
pub mod run;
pub mod view;

use clap::Subcommand;

/// Manage GitHub Actions workflows.
#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    /// Disable a workflow.
    Disable(disable::DisableArgs),
    /// Enable a workflow.
    Enable(enable::EnableArgs),
    /// List workflows.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Run a workflow.
    Run(run::RunArgs),
    /// View a workflow.
    View(view::ViewArgs),
}

impl WorkflowCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Disable(args) => args.run(factory).await,
            Self::Enable(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::Run(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
