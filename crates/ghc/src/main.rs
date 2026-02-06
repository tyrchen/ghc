//! GHC - GitHub CLI written in Rust.
//!
//! Feature-parity rewrite of the official GitHub CLI (`gh`).

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use ghc_cmd::factory::Factory;

/// Exit codes matching the Go CLI behavior.
mod exit_codes {
    pub const OK: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const CANCEL: i32 = 2;
    pub const AUTH: i32 = 4;
    pub const PENDING: i32 = 8;
}

/// GitHub CLI - Work seamlessly with GitHub from the command line.
#[derive(Debug, Parser)]
#[command(
    name = "ghc",
    version,
    about = "GitHub CLI written in Rust",
    long_about = "Work seamlessly with GitHub from the command line."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Manage accessibility settings.
    #[command(subcommand)]
    Accessibility(ghc_cmd::accessibility::AccessibilityCommand),
    /// Learn about working with GitHub Actions.
    Actions(ghc_cmd::actions::ActionsArgs),
    /// Manage AI agent tasks.
    #[command(name = "agent-task", subcommand)]
    AgentTask(ghc_cmd::agent_task::AgentTaskCommand),
    /// Create command shortcuts.
    #[command(subcommand)]
    Alias(ghc_cmd::alias::AliasCommand),
    /// Make an authenticated GitHub API request.
    Api(ghc_cmd::api::ApiArgs),
    /// Manage artifact attestations.
    #[command(subcommand)]
    Attestation(ghc_cmd::attestation::AttestationCommand),
    /// Authenticate gh and git with GitHub.
    #[command(subcommand)]
    Auth(ghc_cmd::auth::AuthCommand),
    /// Open the repository in the web browser.
    Browse(ghc_cmd::browse::BrowseArgs),
    /// Manage GitHub Actions caches.
    #[command(subcommand)]
    Cache(ghc_cmd::cache::CacheCommand),
    /// Connect to and manage codespaces.
    #[command(subcommand)]
    Codespace(ghc_cmd::codespace::CodespaceCommand),
    /// Generate shell completion scripts.
    Completion(ghc_cmd::completion::CompletionArgs),
    /// Manage configuration for ghc.
    #[command(subcommand)]
    Config(ghc_cmd::config::ConfigCommand),
    /// Use GitHub Copilot from the CLI.
    #[command(subcommand)]
    Copilot(ghc_cmd::copilot::CopilotCommand),
    /// Manage extensions.
    #[command(subcommand)]
    Extension(ghc_cmd::extension::ExtensionCommand),
    /// Manage gists.
    #[command(subcommand)]
    Gist(ghc_cmd::gist::GistCommand),
    /// Manage GPG keys.
    #[command(name = "gpg-key", subcommand)]
    GpgKey(ghc_cmd::gpg_key::GpgKeyCommand),
    /// Manage issues.
    #[command(subcommand)]
    Issue(ghc_cmd::issue::IssueCommand),
    /// Manage labels.
    #[command(subcommand)]
    Label(ghc_cmd::label::LabelCommand),
    /// Manage organizations.
    #[command(subcommand)]
    Org(ghc_cmd::org::OrgCommand),
    /// Manage pull requests.
    #[command(subcommand)]
    Pr(ghc_cmd::pr::PrCommand),
    /// Manage feature previews.
    #[command(subcommand)]
    Preview(ghc_cmd::preview::PreviewCommand),
    /// Work with GitHub Projects.
    #[command(subcommand)]
    Project(ghc_cmd::project::ProjectCommand),
    /// Manage releases.
    #[command(subcommand)]
    Release(ghc_cmd::release::ReleaseCommand),
    /// Manage repositories.
    #[command(subcommand)]
    Repo(ghc_cmd::repo::RepoCommand),
    /// View and manage repository rulesets.
    #[command(subcommand)]
    Ruleset(ghc_cmd::ruleset::RulesetCommand),
    /// View details about workflow runs.
    #[command(subcommand)]
    Run(ghc_cmd::run::RunCommand),
    /// Search across GitHub.
    #[command(subcommand)]
    Search(Box<ghc_cmd::search::SearchCommand>),
    /// Manage repository secrets.
    #[command(subcommand)]
    Secret(ghc_cmd::secret::SecretCommand),
    /// Manage SSH keys.
    #[command(name = "ssh-key", subcommand)]
    SshKey(ghc_cmd::ssh_key::SshKeyCommand),
    /// Print information about relevant issues, pull requests, and notifications.
    Status(ghc_cmd::status::StatusArgs),
    /// Manage repository variables.
    #[command(subcommand)]
    Variable(ghc_cmd::variable::VariableCommand),
    /// Show version information.
    Version(ghc_cmd::version::VersionArgs),
    /// View details about GitHub Actions workflows.
    #[command(subcommand)]
    Workflow(ghc_cmd::workflow::WorkflowCommand),
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("GH_DEBUG").unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let factory = Factory::new(env!("CARGO_PKG_VERSION").to_string());

    let exit_code = if let Some(cmd) = cli.command {
        match run_command(cmd, &factory).await {
            Ok(()) => exit_codes::OK,
            Err(e) => {
                if e.downcast_ref::<ghc_core::cmdutil::SilentError>().is_some() {
                    exit_codes::ERROR
                } else if e.downcast_ref::<ghc_core::cmdutil::CancelError>().is_some() {
                    exit_codes::CANCEL
                } else if e.downcast_ref::<ghc_core::cmdutil::AuthError>().is_some() {
                    exit_codes::AUTH
                } else if e
                    .downcast_ref::<ghc_core::cmdutil::PendingError>()
                    .is_some()
                {
                    exit_codes::PENDING
                } else {
                    tracing::error!("{e:#}");
                    exit_codes::ERROR
                }
            }
        }
    } else {
        use clap::CommandFactory;
        Cli::command().print_help().ok();
        println!();
        exit_codes::OK
    };

    std::process::exit(exit_code);
}

async fn run_command(cmd: Commands, factory: &Factory) -> anyhow::Result<()> {
    match cmd {
        Commands::Accessibility(sub) => sub.run(factory).await,
        Commands::Actions(args) => args.run(factory).await,
        Commands::AgentTask(sub) => sub.run(factory).await,
        Commands::Alias(sub) => sub.run(factory),
        Commands::Api(args) => args.run(factory).await,
        Commands::Attestation(sub) => sub.run(factory).await,
        Commands::Auth(sub) => sub.run(factory).await,
        Commands::Browse(args) => args.run(factory).await,
        Commands::Cache(sub) => sub.run(factory).await,
        Commands::Codespace(sub) => sub.run(factory).await,
        Commands::Completion(args) => args.run(factory).await,
        Commands::Config(sub) => sub.run(factory),
        Commands::Copilot(sub) => sub.run(factory).await,
        Commands::Extension(sub) => sub.run(factory).await,
        Commands::Gist(sub) => sub.run(factory).await,
        Commands::GpgKey(sub) => sub.run(factory).await,
        Commands::Issue(sub) => sub.run(factory).await,
        Commands::Label(sub) => sub.run(factory).await,
        Commands::Org(sub) => sub.run(factory).await,
        Commands::Pr(sub) => sub.run(factory).await,
        Commands::Preview(sub) => sub.run(factory).await,
        Commands::Project(sub) => sub.run(factory).await,
        Commands::Release(sub) => sub.run(factory).await,
        Commands::Repo(sub) => sub.run(factory).await,
        Commands::Ruleset(sub) => sub.run(factory).await,
        Commands::Run(sub) => sub.run(factory).await,
        Commands::Search(sub) => sub.run(factory).await,
        Commands::Secret(sub) => sub.run(factory).await,
        Commands::SshKey(sub) => sub.run(factory).await,
        Commands::Status(args) => args.run(factory).await,
        Commands::Variable(sub) => sub.run(factory).await,
        Commands::Version(args) => {
            args.run(
                &factory.io,
                env!("CARGO_PKG_VERSION"),
                option_env!("GHC_BUILD_DATE").unwrap_or(""),
            );
            Ok(())
        }
        Commands::Workflow(sub) => sub.run(factory).await,
    }
}
