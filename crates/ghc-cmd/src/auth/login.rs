//! `ghc auth login` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/login/login.go`.

use std::io::Read;

use clap::Args;
use tracing::info;

use ghc_api::auth_flow;
use ghc_api::http;
use ghc_core::instance;
use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Log in to a GitHub account.
///
/// The default authentication mode is a web-based browser flow.
/// After completion, an authentication token will be stored in
/// the config file. Alternatively, use `--with-token` to pass
/// in a personal access token on standard input.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct LoginArgs {
    /// The hostname of the GitHub instance to authenticate with.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// Additional authentication scopes to request.
    #[arg(short, long, value_delimiter = ',')]
    scopes: Vec<String>,

    /// Read token from standard input.
    #[arg(long = "with-token")]
    with_token: bool,

    /// Open a browser to authenticate.
    #[arg(short, long)]
    web: bool,

    /// Copy one-time OAuth device code to clipboard.
    #[arg(short, long)]
    clipboard: bool,

    /// The protocol to use for git operations on this host.
    #[arg(short = 'p', long, value_parser = ["ssh", "https"])]
    git_protocol: Option<String>,

    /// Save credentials in plain text instead of credential store.
    #[arg(long)]
    insecure_storage: bool,

    /// Skip generate/upload SSH key prompt.
    #[arg(long = "skip-ssh-key")]
    skip_ssh_key: bool,
}

impl LoginArgs {
    /// Run the login command.
    ///
    /// # Errors
    ///
    /// Returns an error if authentication fails.
    #[allow(clippy::await_holding_lock)]
    pub async fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;

        if self.with_token && self.web {
            anyhow::bail!("specify only one of `--web` or `--with-token`");
        }
        if self.with_token && !self.scopes.is_empty() {
            anyhow::bail!("specify only one of `--scopes` or `--with-token`");
        }

        let interactive = ios.can_prompt() && !self.with_token;

        let hostname = match &self.hostname {
            Some(h) => instance::normalize_hostname(h),
            None => {
                if interactive && !self.web {
                    self.prompt_hostname(factory)?
                } else {
                    instance::GITHUB_COM.to_string()
                }
            }
        };

        // Check if token is from environment
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
        let auth = cfg.authentication();
        if let Some((_, source)) = auth.active_token(&hostname) {
            let (_, writeable) = http::auth_token_writeable(&source);
            if !writeable {
                ios_eprintln!(
                    ios,
                    "The value of the {source} environment variable is being used for authentication."
                );
                ios_eprintln!(
                    ios,
                    "To have GitHub CLI store credentials instead, first clear the value from the environment."
                );
                anyhow::bail!("");
            }
        }
        drop(cfg);

        if self.with_token {
            return self.login_with_token(factory, &hostname).await;
        }

        self.login_interactive(factory, &hostname, interactive)
            .await
    }

    async fn login_with_token(&self, factory: &Factory, hostname: &str) -> anyhow::Result<()> {
        let ios = &factory.io;
        let mut token = String::new();
        std::io::stdin().read_to_string(&mut token)?;
        let token = token.trim().to_string();

        if token.is_empty() {
            anyhow::bail!("token cannot be empty");
        }

        // Validate token scopes
        let api_client = factory.api_client(hostname)?;
        api_client
            .has_minimum_scopes(&token)
            .await
            .map_err(|e| anyhow::anyhow!("error validating token: {e}"))?;

        // Get username
        let username = api_client
            .current_login_with_token(&token)
            .await
            .map_err(|e| anyhow::anyhow!("error retrieving current user: {e}"))?;

        // Store credentials
        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
        let git_protocol = self.git_protocol.as_deref().unwrap_or("");
        let secure_storage = !self.insecure_storage;
        cfg.authentication_mut().login(
            hostname,
            &username,
            &token,
            git_protocol,
            secure_storage,
        )?;

        info!(hostname, username, "Logged in with token");
        ios_eprintln!(ios, "Logged in as {username}");
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn login_interactive(
        &self,
        factory: &Factory,
        hostname: &str,
        interactive: bool,
    ) -> anyhow::Result<()> {
        let ios = &factory.io;
        let git_protocol = match &self.git_protocol {
            Some(p) => p.to_lowercase(),
            None => {
                if interactive {
                    let options = vec!["HTTPS".to_string(), "SSH".to_string()];
                    let prompter = factory.prompter();
                    let idx = prompter.select(
                        "What is your preferred protocol for Git operations on this host?",
                        Some(0),
                        &options,
                    )?;
                    options[idx].to_lowercase()
                } else {
                    "https".to_string()
                }
            }
        };

        // Choose auth mode
        let use_web = if self.web {
            true
        } else if interactive {
            let options = vec![
                "Login with a web browser".to_string(),
                "Paste an authentication token".to_string(),
            ];
            let prompter = factory.prompter();
            let idx = prompter.select(
                "How would you like to authenticate GitHub CLI?",
                Some(0),
                &options,
            )?;
            idx == 0
        } else {
            true
        };

        let (token, username) = if use_web {
            let mut scopes: Vec<&str> = auth_flow::DEFAULT_SCOPES.to_vec();
            for s in &self.scopes {
                scopes.push(s.as_str());
            }

            let browser = factory.browser();
            let http = ghc_api::http::build_client(&ghc_api::http::HttpClientOptions {
                app_version: factory.app_version.clone(),
                skip_default_headers: false,
                log_verbose: false,
            })?;

            let result = auth_flow::auth_flow(
                &http,
                hostname,
                &scopes,
                browser.as_ref(),
                self.clipboard,
                &mut std::io::stderr(),
            )
            .await?;

            ios_eprintln!(ios, "Authentication complete.");
            (result.token, result.username)
        } else {
            // Token auth mode
            let minimum_scopes = ["repo", "read:org"];
            ios_eprintln!(
                ios,
                "Tip: you can generate a Personal Access Token here https://{hostname}/settings/tokens"
            );
            ios_eprintln!(
                ios,
                "The minimum required scopes are {}.",
                minimum_scopes
                    .iter()
                    .map(|s| format!("'{s}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            let prompter = factory.prompter();
            let token = prompter.password("Paste your authentication token:")?;

            if token.is_empty() {
                anyhow::bail!("token cannot be empty");
            }

            // Validate token
            let api_client = factory.api_client(hostname)?;
            api_client
                .has_minimum_scopes(&token)
                .await
                .map_err(|e| anyhow::anyhow!("error validating token: {e}"))?;

            let username = api_client
                .current_login_with_token(&token)
                .await
                .map_err(|e| anyhow::anyhow!("error retrieving current user: {e}"))?;

            (token, username)
        };

        // Store git protocol
        if !git_protocol.is_empty() {
            ios_eprintln!(
                ios,
                "- ghc config set -h {hostname} git_protocol {git_protocol}"
            );
            ios_eprintln!(ios, "Configured git protocol");
        }

        // Store credentials
        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;
        let secure_storage = !self.insecure_storage;
        cfg.authentication_mut().login(
            hostname,
            &username,
            &token,
            &git_protocol,
            secure_storage,
        )?;

        ios_eprintln!(ios, "Logged in as {username}");
        Ok(())
    }

    #[allow(clippy::unused_self)]
    fn prompt_hostname(&self, factory: &Factory) -> anyhow::Result<String> {
        let options = vec!["GitHub.com".to_string(), "Other".to_string()];
        let prompter = factory.prompter();
        let idx = prompter.select("Where do you use GitHub?", Some(0), &options)?;

        if idx == 0 {
            Ok(instance::GITHUB_COM.to_string())
        } else {
            prompter.input("Hostname:", "")
        }
    }
}
