//! Authentication commands for GHC.
//!
//! Maps from Go's `pkg/cmd/auth/` package. Provides login, logout,
//! status, token, switch, setup-git, and git-credential subcommands.

pub mod git_credential;
pub mod login;
pub mod logout;
pub mod refresh;
pub mod setup_git;
pub mod status;
pub mod switch;
pub mod token;

use clap::Subcommand;

use crate::factory::Factory;

/// Auth subcommands.
#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Log in to a GitHub account.
    Login(login::LoginArgs),
    /// Log out of a GitHub account.
    Logout(logout::LogoutArgs),
    /// Refresh stored authentication credentials.
    Refresh(refresh::RefreshArgs),
    /// Display active account and authentication state.
    Status(status::StatusArgs),
    /// Print the authentication token for a hostname.
    Token(token::TokenArgs),
    /// Switch active GitHub account.
    Switch(switch::SwitchArgs),
    /// Configure git to use GitHub CLI as credential helper.
    SetupGit(setup_git::SetupGitArgs),
    /// Implement git credential helper protocol.
    #[command(name = "git-credential", hide = true)]
    GitCredential(git_credential::GitCredentialArgs),
}

impl AuthCommand {
    /// Run the appropriate auth subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(self, factory: &Factory) -> anyhow::Result<()> {
        match self {
            Self::Login(args) => args.run(factory).await,
            Self::Logout(args) => args.run(factory).await,
            Self::Refresh(args) => args.run(factory).await,
            Self::Status(args) => args.run(factory).await,
            Self::Token(args) => args.run(factory),
            Self::Switch(args) => args.run(factory),
            Self::SetupGit(args) => args.run(factory),
            Self::GitCredential(args) => args.run(factory),
        }
    }
}
