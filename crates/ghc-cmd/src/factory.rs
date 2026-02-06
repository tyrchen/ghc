//! Factory for shared command dependencies.
//!
//! Maps from Go's `pkg/cmd/factory` package. Provides lazy initialization
//! of configuration, API clients, browser, and prompter. Supports test mode
//! with dependency injection for isolated testing.

use std::sync::{Arc, Mutex, OnceLock};

use ghc_core::browser::{Browser, StubBrowser, SystemBrowser};
use ghc_core::config::{Config, FileConfig};
use ghc_core::iostreams::{IOStreams, TestOutput};
use ghc_core::prompter::{DialoguerPrompter, Prompter, StubPrompter};
use ghc_git::client::GitClient;
use secrecy::SecretString;

/// Shared factory providing lazily-initialized dependencies to all commands.
///
/// In production mode, dependencies are created from the real system.
/// In test mode, dependencies can be injected for isolated testing.
pub struct Factory {
    /// Application version.
    pub app_version: String,
    /// I/O streams.
    pub io: IOStreams,
    /// Configuration (lazily loaded).
    config: OnceLock<Mutex<Box<dyn Config>>>,
    /// Git client (lazily loaded).
    git_client: OnceLock<GitClient>,

    // Test overrides
    http_override: Option<reqwest::Client>,
    api_url_override: Option<String>,
    token_override: Option<SecretString>,
    browser_stub: Option<Arc<StubBrowser>>,
    prompter_stub: Option<Arc<StubPrompter>>,
}

impl std::fmt::Debug for Factory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Factory")
            .field("app_version", &self.app_version)
            .finish_non_exhaustive()
    }
}

impl Factory {
    /// Create a new factory with the given version.
    pub fn new(app_version: String) -> Self {
        let mut io = IOStreams::system();

        // Apply environment overrides
        if std::env::var("GH_PROMPT_DISABLED").is_ok() {
            io.set_never_prompt(true);
        }

        Self {
            app_version,
            io,
            config: OnceLock::new(),
            git_client: OnceLock::new(),
            http_override: None,
            api_url_override: None,
            token_override: None,
            browser_stub: None,
            prompter_stub: None,
        }
    }

    /// Create a test factory with captured I/O and in-memory config.
    ///
    /// Returns the factory and a `TestOutput` for reading captured
    /// stdout/stderr.
    pub fn test() -> (Self, TestOutput) {
        let (io, output) = IOStreams::test_with_output();

        let factory = Self {
            app_version: "test".to_string(),
            io,
            config: OnceLock::new(),
            git_client: OnceLock::new(),
            http_override: None,
            api_url_override: None,
            token_override: None,
            browser_stub: None,
            prompter_stub: None,
        };

        (factory, output)
    }

    /// Set a custom reqwest HTTP client (e.g., backed by wiremock).
    #[must_use]
    pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
        self.http_override = Some(client);
        self
    }

    /// Set an API URL override (wiremock server URI with trailing slash).
    ///
    /// When set, all API requests (REST and GraphQL) will be sent to
    /// this base URL instead of the real GitHub API.
    #[must_use]
    pub fn with_api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url_override = Some(url.into());
        self
    }

    /// Set a test auth token.
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token_override = Some(SecretString::from(token.into()));
        self
    }

    /// Set a config override for testing.
    #[must_use]
    pub fn with_config(self, config: Box<dyn Config>) -> Self {
        let _ = self.config.set(Mutex::new(config));
        self
    }

    /// Set a stub browser and return the shared reference for verification.
    pub fn with_stub_browser(mut self) -> (Self, Arc<StubBrowser>) {
        let stub = Arc::new(StubBrowser::default());
        self.browser_stub = Some(stub.clone());
        (self, stub)
    }

    /// Set a stub prompter and return the shared reference for configuration.
    pub fn with_stub_prompter(mut self) -> (Self, Arc<StubPrompter>) {
        let stub = Arc::new(StubPrompter::default());
        self.prompter_stub = Some(stub.clone());
        (self, stub)
    }

    /// Get the configuration, loading it if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if config cannot be loaded.
    pub fn config(&self) -> anyhow::Result<&Mutex<Box<dyn Config>>> {
        if let Some(cfg) = self.config.get() {
            return Ok(cfg);
        }
        let cfg = FileConfig::load()?;
        let boxed: Box<dyn Config> = Box::new(cfg);
        // Ignore set error - another thread may have set it first
        let _ = self.config.set(Mutex::new(boxed));
        self.config
            .get()
            .ok_or_else(|| anyhow::anyhow!("failed to initialize config"))
    }

    /// Get the git client.
    ///
    /// # Errors
    ///
    /// Returns an error if git is not available.
    pub fn git_client(&self) -> anyhow::Result<&GitClient> {
        if let Some(client) = self.git_client.get() {
            return Ok(client);
        }
        let client = GitClient::new()?;
        let _ = self.git_client.set(client);
        self.git_client
            .get()
            .ok_or_else(|| anyhow::anyhow!("failed to initialize git client"))
    }

    /// Create a browser instance.
    ///
    /// In test mode with a stub browser, returns the stub.
    pub fn browser(&self) -> Box<dyn Browser> {
        if let Some(ref stub) = self.browser_stub {
            return Box::new(StubBrowserWrapper(stub.clone()));
        }
        if let Ok(cfg_lock) = self.config()
            && let Ok(cfg) = cfg_lock.lock()
            && let Some(launcher) = cfg.browser("")
        {
            return Box::new(SystemBrowser::with_launcher(launcher));
        }
        Box::new(SystemBrowser::new())
    }

    /// Create a prompter instance.
    ///
    /// In test mode with a stub prompter, returns the stub.
    pub fn prompter(&self) -> Box<dyn Prompter> {
        if let Some(ref stub) = self.prompter_stub {
            return Box::new(StubPrompterWrapper(stub.clone()));
        }
        let editor = self.config().ok().and_then(|c| c.lock().ok()?.editor(""));
        Box::new(DialoguerPrompter::new(editor))
    }

    /// Build an HTTP client for API requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be built.
    pub fn http_client(&self) -> anyhow::Result<ghc_api::client::Client> {
        let hostname = "github.com"; // default
        self.api_client(hostname)
    }

    /// Build an API client for a specific hostname.
    ///
    /// In test mode, uses the injected HTTP client and URL override.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be built or auth is missing.
    pub fn api_client(&self, hostname: &str) -> anyhow::Result<ghc_api::client::Client> {
        let http = if let Some(ref client) = self.http_override {
            client.clone()
        } else {
            let opts = ghc_api::http::HttpClientOptions {
                app_version: self.app_version.clone(),
                skip_default_headers: false,
                log_verbose: std::env::var("GH_DEBUG").is_ok(),
            };
            ghc_api::http::build_client(&opts)?
        };

        let token: Option<SecretString> = self.token_override.clone().or_else(|| {
            self.config().ok().and_then(|c| {
                let cfg = c.lock().ok()?;
                cfg.authentication()
                    .active_token(hostname)
                    .map(|(t, _)| SecretString::from(t))
            })
        });

        let mut client = ghc_api::client::Client::new(http, hostname, token);
        if let Some(ref url) = self.api_url_override {
            client = client.with_url_override(url.clone());
        }
        Ok(client)
    }
}

/// Wrapper to use `Arc<StubBrowser>` as `Box<dyn Browser>`.
#[derive(Debug)]
struct StubBrowserWrapper(Arc<StubBrowser>);

impl Browser for StubBrowserWrapper {
    fn open(&self, url: &str) -> anyhow::Result<()> {
        self.0.open(url)
    }
}

/// Wrapper to use `Arc<StubPrompter>` as `Box<dyn Prompter>`.
#[derive(Debug)]
struct StubPrompterWrapper(Arc<StubPrompter>);

impl Prompter for StubPrompterWrapper {
    fn select(
        &self,
        prompt: &str,
        default: Option<usize>,
        options: &[String],
    ) -> anyhow::Result<usize> {
        self.0.select(prompt, default, options)
    }

    fn multi_select(
        &self,
        prompt: &str,
        defaults: &[bool],
        options: &[String],
    ) -> anyhow::Result<Vec<usize>> {
        self.0.multi_select(prompt, defaults, options)
    }

    fn input(&self, prompt: &str, default: &str) -> anyhow::Result<String> {
        self.0.input(prompt, default)
    }

    fn password(&self, prompt: &str) -> anyhow::Result<String> {
        self.0.password(prompt)
    }

    fn confirm(&self, prompt: &str, default: bool) -> anyhow::Result<bool> {
        self.0.confirm(prompt, default)
    }

    fn editor(&self, prompt: &str, default: &str, allow_blank: bool) -> anyhow::Result<String> {
        self.0.editor(prompt, default, allow_blank)
    }
}
