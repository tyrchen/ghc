//! `ghc auth refresh` command implementation.
//!
//! Maps from Go's `pkg/cmd/auth/refresh/refresh.go`.
//! Refreshes stored authentication credentials by re-running the OAuth flow
//! with updated scopes.

use std::collections::BTreeSet;

use clap::Args;

use ghc_api::auth_flow;
use ghc_api::http;
use ghc_core::ios_eprintln;

use crate::factory::Factory;

/// Refresh stored authentication credentials.
///
/// Expand or fix the permission scopes for stored credentials for the active account.
/// The `--scopes` flag accepts a comma-separated list of scopes you want your
/// credentials to have. If no scopes are provided, the command maintains
/// previously added scopes.
///
/// The `--remove-scopes` flag accepts a comma-separated list of scopes you want
/// to remove. Scope removal is idempotent. The minimum set of scopes
/// (`repo`, `read:org`, and `gist`) cannot be removed.
///
/// The `--reset-scopes` flag resets scopes to the default minimum set.
#[derive(Debug, Args)]
pub struct RefreshArgs {
    /// The GitHub host to use for authentication.
    #[arg(short = 'h', long)]
    hostname: Option<String>,

    /// Additional authentication scopes for ghc to have.
    #[arg(short, long, value_delimiter = ',')]
    scopes: Vec<String>,

    /// Authentication scopes to remove from ghc.
    #[arg(short = 'r', long = "remove-scopes", value_delimiter = ',')]
    remove_scopes: Vec<String>,

    /// Reset authentication scopes to the default minimum set.
    #[arg(long = "reset-scopes")]
    reset_scopes: bool,

    /// Copy one-time OAuth device code to clipboard.
    #[arg(short, long)]
    clipboard: bool,

    /// Save authentication credentials in plain text instead of credential store.
    #[arg(long)]
    insecure_storage: bool,
}

impl RefreshArgs {
    /// Run the refresh command.
    ///
    /// # Errors
    ///
    /// Returns an error if the credential refresh fails.
    pub async fn run(&self, factory: &Factory) -> anyhow::Result<()> {
        let ios = &factory.io;
        let interactive = ios.can_prompt();

        if !interactive && self.hostname.is_none() {
            anyhow::bail!("--hostname required when not running interactively");
        }

        let (hostname, old_token) = self.resolve_host_and_token(factory)?;

        check_token_writeable(factory, &hostname)?;

        let additional_scopes = self
            .build_scopes(factory, &hostname, old_token.as_deref())
            .await?;

        let result = self
            .run_oauth_flow(factory, &hostname, &additional_scopes)
            .await?;

        self.verify_and_store(factory, &hostname, &result)?;

        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Authentication complete.", cs.success_icon());

        Ok(())
    }

    /// Resolve which host to refresh and retrieve the current token.
    fn resolve_host_and_token(
        &self,
        factory: &Factory,
    ) -> anyhow::Result<(String, Option<String>)> {
        let cfg_lock = factory.config()?;
        let cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let candidates = cfg.authentication().hosts();
        if candidates.is_empty() {
            anyhow::bail!(
                "not logged in to any hosts. Use 'ghc auth login' to authenticate with a host"
            );
        }

        let hostname = if let Some(ref h) = self.hostname {
            if !candidates.contains(h) {
                anyhow::bail!(
                    "not logged in to {h}. Use 'ghc auth login' to authenticate with this host"
                );
            }
            h.clone()
        } else if candidates.len() == 1 {
            candidates[0].clone()
        } else {
            let prompter = factory.prompter();
            let selected = prompter.select(
                "What account do you want to refresh auth for?",
                None,
                &candidates,
            )?;
            candidates[selected].clone()
        };

        let old_token = if self.reset_scopes {
            None
        } else {
            cfg.authentication().active_token(&hostname).map(|(t, _)| t)
        };

        Ok((hostname, old_token))
    }

    /// Build the set of scopes for the OAuth flow.
    async fn build_scopes(
        &self,
        factory: &Factory,
        hostname: &str,
        old_token: Option<&str>,
    ) -> anyhow::Result<BTreeSet<String>> {
        let mut additional_scopes: BTreeSet<String> = BTreeSet::new();

        if let Some(old_token) = old_token {
            let api_client = factory.api_client(hostname)?;
            if let Ok(old_scopes) = api_client.get_scopes(old_token).await {
                for s in old_scopes.split(',') {
                    let s = s.trim();
                    if !s.is_empty() {
                        additional_scopes.insert(s.to_string());
                    }
                }
            }
        }

        for s in &self.scopes {
            additional_scopes.insert(s.clone());
        }
        for s in &self.remove_scopes {
            additional_scopes.remove(s);
        }

        Ok(additional_scopes)
    }

    /// Run the OAuth flow with the given scopes.
    async fn run_oauth_flow(
        &self,
        factory: &Factory,
        hostname: &str,
        additional_scopes: &BTreeSet<String>,
    ) -> anyhow::Result<auth_flow::AuthFlowResult> {
        let scope_refs: Vec<&str> = additional_scopes.iter().map(String::as_str).collect();
        let browser = factory.browser();
        let http_client = ghc_api::http::build_client(&ghc_api::http::HttpClientOptions {
            app_version: factory.app_version.clone(),
            skip_default_headers: false,
            log_verbose: false,
        })?;

        auth_flow::auth_flow(
            &http_client,
            hostname,
            &scope_refs,
            browser.as_ref(),
            self.clipboard,
            &mut std::io::stderr(),
        )
        .await
    }

    /// Verify the authenticated user and store new credentials.
    #[allow(clippy::unused_self)]
    fn verify_and_store(
        &self,
        factory: &Factory,
        hostname: &str,
        result: &auth_flow::AuthFlowResult,
    ) -> anyhow::Result<()> {
        let cfg_lock = factory.config()?;
        let mut cfg = cfg_lock
            .lock()
            .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

        let active_user = cfg.authentication().active_user(hostname);
        if let Some(ref active) = active_user
            && !active.is_empty()
            && active != &result.username
        {
            anyhow::bail!(
                "error refreshing credentials for {active}, received credentials for {}, did you use the correct account in the browser?",
                result.username
            );
        }

        cfg.authentication_mut()
            .login(hostname, &result.username, &result.token, "")?;
        Ok(())
    }
}

/// Check if the token for this host is writeable (not from an env var).
fn check_token_writeable(factory: &Factory, hostname: &str) -> anyhow::Result<()> {
    let cfg_lock = factory.config()?;
    let cfg = cfg_lock
        .lock()
        .map_err(|e| anyhow::anyhow!("config lock: {e}"))?;

    if let Some((_, source)) = cfg.authentication().active_token(hostname) {
        let (_, writeable) = http::auth_token_writeable(&source);
        if !writeable {
            let ios = &factory.io;
            ios_eprintln!(
                ios,
                "The value of the {source} environment variable is being used for authentication."
            );
            ios_eprintln!(
                ios,
                "To refresh credentials stored in GitHub CLI, first clear the value from the environment."
            );
            anyhow::bail!("");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use ghc_core::config::MemoryConfig;

    use crate::test_helpers::TestHarness;

    #[tokio::test]
    async fn test_should_error_when_not_logged_in() {
        let config = MemoryConfig::new();
        let h = TestHarness::with_config(config).await;
        let args = RefreshArgs {
            hostname: Some("github.com".to_string()),
            scopes: Vec::new(),
            remove_scopes: Vec::new(),
            reset_scopes: false,
            clipboard: false,
            insecure_storage: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not logged in"));
    }

    #[tokio::test]
    async fn test_should_error_for_unknown_host() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let h = TestHarness::with_config(config).await;
        let args = RefreshArgs {
            hostname: Some("unknown.host".to_string()),
            scopes: Vec::new(),
            remove_scopes: Vec::new(),
            reset_scopes: false,
            clipboard: false,
            insecure_storage: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not logged in to unknown.host")
        );
    }

    #[tokio::test]
    async fn test_should_require_hostname_in_non_interactive() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        // TestHarness creates non-interactive IO by default
        let h = TestHarness::with_config(config).await;
        let args = RefreshArgs {
            hostname: None,
            scopes: Vec::new(),
            remove_scopes: Vec::new(),
            reset_scopes: false,
            clipboard: false,
            insecure_storage: false,
        };
        // Single host auto-selects, so this should proceed (not error about --hostname)
        // The actual error will be from the OAuth flow since we don't mock it
        let result = args.run(&h.factory).await;
        // It should try to proceed with the single host, then fail at OAuth
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_should_error_with_env_token() {
        let config = MemoryConfig::new().with_host("github.com", "testuser", "ghp_abc");
        let (factory, _output) = crate::factory::Factory::test();
        let (factory, _browser) = factory.with_stub_browser();
        let (factory, _prompter) = factory.with_stub_prompter();

        // Create a custom config that returns env source
        let factory = factory.with_config(Box::new(EnvTokenConfig));

        let args = RefreshArgs {
            hostname: Some("github.com".to_string()),
            scopes: Vec::new(),
            remove_scopes: Vec::new(),
            reset_scopes: false,
            clipboard: false,
            insecure_storage: false,
        };
        let result = args.run(&factory).await;
        assert!(result.is_err());
        let _ = config; // suppress unused warning
    }

    /// A test config that simulates a token from an environment variable.
    #[derive(Debug)]
    struct EnvTokenConfig;

    impl ghc_core::config::Config for EnvTokenConfig {
        fn get(&self, _hostname: &str, _key: &str) -> Option<String> {
            None
        }
        fn get_or_default(&self, _hostname: &str, key: &str) -> String {
            ghc_core::config::default_for_key(key).to_string()
        }
        fn set(&mut self, _hostname: &str, _key: &str, _value: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn aliases(&self) -> &std::collections::HashMap<String, String> {
            static EMPTY: std::sync::LazyLock<std::collections::HashMap<String, String>> =
                std::sync::LazyLock::new(std::collections::HashMap::new);
            &EMPTY
        }
        fn set_alias(&mut self, _name: &str, _expansion: &str) {}
        fn delete_alias(&mut self, _name: &str) -> Option<String> {
            None
        }
        fn hosts(&self) -> Vec<String> {
            vec!["github.com".to_string()]
        }
        fn authentication(&self) -> &dyn ghc_core::config::AuthConfig {
            self
        }
        fn authentication_mut(&mut self) -> &mut dyn ghc_core::config::AuthConfig {
            self
        }
        fn write(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl ghc_core::config::AuthConfig for EnvTokenConfig {
        fn active_token(&self, _hostname: &str) -> Option<(String, String)> {
            Some(("ghp_env_token".to_string(), "GH_TOKEN".to_string()))
        }
        fn active_user(&self, _hostname: &str) -> Option<String> {
            Some("envuser".to_string())
        }
        fn hosts(&self) -> Vec<String> {
            vec!["github.com".to_string()]
        }
        fn login(
            &mut self,
            _hostname: &str,
            _username: &str,
            _token: &str,
            _git_protocol: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn logout(&mut self, _hostname: &str, _username: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn switch_user(&mut self, _hostname: &str, _username: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn users_for_host(&self, _hostname: &str) -> Vec<String> {
            vec!["envuser".to_string()]
        }
        fn token_for_user(&self, _hostname: &str, _username: &str) -> Option<(String, String)> {
            Some(("ghp_env_token".to_string(), "GH_TOKEN".to_string()))
        }
    }
}
