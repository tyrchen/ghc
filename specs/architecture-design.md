# GHC Architecture Design Specification

## Overview

GHC is a complete Rust rewrite of the official GitHub CLI (`gh`). The Go reference implementation (at `vendors/cli/`) consists of ~475 source files, ~92K lines of code, and 35 top-level command groups. This specification defines the Rust architecture, workspace layout, module structure, key traits, dependency choices, and design patterns to achieve full feature parity.

## 1. Workspace Layout

The project uses a Cargo workspace with multiple crates to enforce clean dependency boundaries and enable parallel compilation. We use a more granular crate split than the Go version to enforce separation of concerns at the compile level.

```
ghc/
  Cargo.toml              # Workspace root
  rust-toolchain.toml     # Pin Rust 2024 edition, stable channel
  deny.toml               # cargo-deny configuration
  Makefile                 # Build automation
  CLAUDE.md               # Development guidelines
  specs/                   # Design specs
  docs/                    # Documentation
  vendors/cli/             # Go reference implementation (submodule)
  crates/
    ghc/                   # Binary crate - CLI entry point
      Cargo.toml
      src/
        main.rs            # Entry point, exit code handling, update checker
    ghc-cli/               # CLI command definitions (clap) + Factory
      Cargo.toml
      src/
        lib.rs
        factory.rs          # Factory struct with lazy deps (OnceCell)
        root.rs             # Root command registration, help topics
        auth_check.rs       # Pre-run auth check middleware
        flags.rs            # Shared flag helpers (json, format, web)
        update.rs           # Background update checker
        commands/            # Command modules (see Section 4)
          mod.rs
          repo/
          issue/
          pr/
          auth/
          config/
          gist/
          release/
          run/
          workflow/
          cache/
          search/
          secret/
          variable/
          label/
          ssh_key/
          gpg_key/
          alias/
          api/
          browse/
          codespace/
          copilot/
          extension/
          org/
          project/
          ruleset/
          attestation/
          status/
          version/
          completion/
          actions/
          accessibility/
          preview/
          agent_task/
    ghc-core/              # Core domain types and traits
      Cargo.toml
      src/
        lib.rs
        config.rs           # Config, AuthConfig, AliasConfig traits
        repo.rs             # Repository trait + Repo struct
        remote.rs           # Remote types, sorting, translation
        instance.rs         # Hostname normalization, API endpoint generation
        error.rs            # Domain error types (SilentError, CancelError, etc.)
    ghc-api/               # GitHub API client (REST + GraphQL)
      Cargo.toml
      src/
        lib.rs
        client.rs           # GitHubClient (REST + GraphQL)
        graphql.rs           # GraphQL execution helpers
        rest.rs              # REST execution helpers
        http.rs              # HTTP client construction, auth middleware
        error.rs             # ApiError, HTTPError, GraphQLError
        feature.rs           # API feature detection
        queries/             # Domain-specific queries
          mod.rs
          issue.rs
          pr.rs
          repo.rs
          user.rs
          org.rs
          project.rs
          search.rs
          comments.rs
          review.rs
          branch.rs
    ghc-git/               # Git client wrapper
      Cargo.toml
      src/
        lib.rs
        client.rs           # GitClient - subprocess wrapper around `git`
        remote.rs            # Remote parsing from `git remote -v`
        url.rs               # Git URL parsing and SSH/HTTPS translation
        command.rs           # GitCommand wrapper with output handling
        error.rs             # Git error types
    ghc-config/            # Configuration implementation
      Cargo.toml
      src/
        lib.rs
        config.rs            # YamlConfig implementing Config trait
        auth_config.rs       # AuthConfig impl (token resolution, login/logout/switch)
        alias_config.rs      # AliasConfig impl
        migration.rs         # Config schema migration system
        keyring.rs           # Secure credential storage via keyring crate
        defaults.rs          # Default config values and config options
        error.rs             # Config error types
    ghc-term/              # Terminal I/O, prompting, formatting
      Cargo.toml
      src/
        lib.rs
        iostreams.rs         # IOStreams struct (stdin/stdout/stderr, TTY, pager)
        color.rs             # ColorScheme, ANSI support, theme detection
        pager.rs             # Pager process management
        spinner.rs           # Progress indicators (spinner + textual fallback)
        prompter.rs          # Prompter trait + dialoguer/accessible impls
        browser.rs           # Browser trait + open crate impl
        table.rs             # TablePrinter using comfy-table
        markdown.rs          # Markdown rendering using termimad
        export.rs            # JSON/jq/template export (Exporter trait)
        json_color.rs        # Colorized JSON output
        text.rs              # Text utilities (truncation, fuzzy time, padding, pluralize)
        error.rs             # Terminal error types
    ghc-auth/              # Authentication flows
      Cargo.toml
      src/
        lib.rs
        oauth.rs             # OAuth device flow implementation
        token.rs             # Token validation and viewer lookup
        error.rs             # Auth flow error types
    ghc-ext/               # Extension system
      Cargo.toml
      src/
        lib.rs
        extension.rs         # Extension trait + InstalledExtension types
        manager.rs           # ExtensionManager impl
        error.rs             # Extension error types
    ghc-search/            # Search query builder
      Cargo.toml
      src/
        lib.rs
        query.rs             # Query builder (qualifiers, pagination)
        result.rs            # Result types (repos, issues, PRs, commits, code)
        searcher.rs          # Searcher trait + impl
        error.rs             # Search error types
  tests/                   # Integration tests
    cli_integration.rs
    api_integration.rs
    auth_integration.rs
    parity/                # Feature parity test suite vs Go `gh`
```

### Crate Dependency Graph

```
ghc (binary)
  -> ghc-cli
  -> ghc-term (IOStreams initialization)
  -> ghc-config (config initialization)

ghc-cli
  -> ghc-core
  -> ghc-api
  -> ghc-git
  -> ghc-config
  -> ghc-term
  -> ghc-auth
  -> ghc-ext
  -> ghc-search

ghc-api    -> ghc-core
ghc-git    -> ghc-core
ghc-config -> ghc-core
ghc-term   -> ghc-core
ghc-auth   -> ghc-core, ghc-api, ghc-term
ghc-ext    -> ghc-core, ghc-api, ghc-git
ghc-search -> ghc-core, ghc-api
```

## 2. Key Trait Definitions

### 2.1 IOStreams

The central terminal I/O abstraction, matching Go's `pkg/iostreams/iostreams.go`. Manages stdin/stdout/stderr, TTY detection, color support, pager processes, and progress indicators.

```rust
/// Terminal I/O abstraction for all CLI output.
#[derive(Debug)]
pub struct IOStreams {
    stdin: Box<dyn Read + Send>,
    stdout: Box<dyn Write + Send>,
    stderr: Box<dyn Write + Send>,

    // TTY state
    stdin_is_tty: bool,
    stdout_is_tty: bool,
    stderr_is_tty: bool,

    // Color
    color_enabled: bool,
    color_256: bool,
    true_color: bool,
    color_labels: bool,
    accessible_colors: bool,

    // Terminal
    terminal_width: u16,
    terminal_theme: TerminalTheme,

    // Pager
    pager_command: String,
    pager_process: Option<std::process::Child>,

    // Spinner
    spinner_disabled: bool,
    progress_indicator_enabled: bool,

    // Prompts
    never_prompt: bool,
    accessible_prompter: bool,

    // Alternate screen buffer
    alternate_screen_buffer_enabled: bool,
    alternate_screen_buffer_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalTheme {
    Light,
    Dark,
    None,
}

impl IOStreams {
    /// Create IOStreams connected to real system streams with TTY detection.
    pub fn system() -> Self;
    /// Create IOStreams with in-memory buffers for testing.
    pub fn test() -> (Self, Vec<u8>, Vec<u8>, Vec<u8>);

    // TTY
    pub fn is_stdin_tty(&self) -> bool;
    pub fn is_stdout_tty(&self) -> bool;
    pub fn is_stderr_tty(&self) -> bool;

    // Color
    pub fn color_enabled(&self) -> bool;
    pub fn color_support_256(&self) -> bool;
    pub fn has_true_color(&self) -> bool;
    pub fn color_scheme(&self) -> ColorScheme;

    // Pager
    pub fn start_pager(&mut self) -> Result<()>;
    pub fn stop_pager(&mut self);

    // Spinner
    pub fn start_progress_indicator(&mut self);
    pub fn start_progress_indicator_with_label(&mut self, label: &str);
    pub fn stop_progress_indicator(&mut self);

    // Alternate screen buffer
    pub fn start_alternate_screen_buffer(&mut self);
    pub fn stop_alternate_screen_buffer(&mut self);

    // Misc
    pub fn can_prompt(&self) -> bool;
    pub fn terminal_width(&self) -> u16;
    pub fn read_user_file(&self, filename: &str) -> Result<Vec<u8>>;
}
```

### 2.2 Config Trait

Maps Go's `internal/gh/gh.go` Config interface. Object-safe for `Arc<dyn Config>`.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    User,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConfigEntry {
    pub value: String,
    pub source: ConfigSource,
}

/// Application configuration.
/// Uses async-trait because object safety is required for Arc<dyn Config>.
#[async_trait]
pub trait Config: Send + Sync + std::fmt::Debug {
    fn get_or_default(&self, hostname: &str, key: &str) -> Option<ConfigEntry>;
    fn set(&mut self, hostname: &str, key: &str, value: &str);

    // Typed accessors (all return ConfigEntry with source tracking)
    fn browser(&self, hostname: &str) -> ConfigEntry;
    fn editor(&self, hostname: &str) -> ConfigEntry;
    fn git_protocol(&self, hostname: &str) -> ConfigEntry;
    fn http_unix_socket(&self, hostname: &str) -> ConfigEntry;
    fn pager(&self, hostname: &str) -> ConfigEntry;
    fn prompt(&self, hostname: &str) -> ConfigEntry;
    fn prefer_editor_prompt(&self, hostname: &str) -> ConfigEntry;
    fn spinner(&self, hostname: &str) -> ConfigEntry;
    fn color_labels(&self, hostname: &str) -> ConfigEntry;
    fn accessible_colors(&self, hostname: &str) -> ConfigEntry;
    fn accessible_prompter(&self, hostname: &str) -> ConfigEntry;

    fn aliases(&self) -> &dyn AliasConfig;
    fn authentication(&self) -> &dyn AuthConfig;

    fn cache_dir(&self) -> &str;
    fn migrate(&mut self, migration: &dyn Migration) -> Result<()>;
    fn version(&self) -> Option<&str>;
    fn write(&self) -> Result<()>;
}
```

### 2.3 AuthConfig Trait

Maps Go's `internal/gh/gh.go` AuthConfig interface.

```rust
/// Authentication configuration for managing tokens and hosts.
#[async_trait]
pub trait AuthConfig: Send + Sync + std::fmt::Debug {
    fn has_active_token(&self, hostname: &str) -> bool;
    /// Returns (token, source) where source is "env", "keyring", or "oauth_token".
    fn active_token(&self, hostname: &str) -> (String, String);
    fn has_env_token(&self) -> bool;
    fn token_from_keyring(&self, hostname: &str) -> Result<String>;
    fn token_from_keyring_for_user(&self, hostname: &str, username: &str) -> Result<String>;
    fn active_user(&self, hostname: &str) -> Result<String>;
    fn hosts(&self) -> Vec<String>;
    /// Returns (host, source) where source is "GH_HOST", "default", etc.
    fn default_host(&self) -> (String, String);
    /// Returns true if insecure storage was used (keyring unavailable).
    fn login(
        &mut self,
        hostname: &str,
        username: &str,
        token: &str,
        git_protocol: &str,
        secure_storage: bool,
    ) -> Result<bool>;
    fn switch_user(&mut self, hostname: &str, user: &str) -> Result<()>;
    fn logout(&mut self, hostname: &str, username: &str) -> Result<()>;
    fn users_for_host(&self, hostname: &str) -> Vec<String>;
    /// Returns (token, source, error).
    fn token_for_user(&self, hostname: &str, user: &str) -> Result<(String, String)>;
}
```

### 2.4 AliasConfig Trait

```rust
pub trait AliasConfig: Send + Sync + std::fmt::Debug {
    fn get(&self, alias: &str) -> Result<String>;
    fn add(&mut self, alias: &str, expansion: &str);
    fn delete(&mut self, alias: &str) -> Result<()>;
    fn all(&self) -> HashMap<String, String>;
}
```

### 2.5 Repository Trait

Maps Go's `internal/ghrepo/repo.go` Interface.

```rust
pub trait Repository: Send + Sync + std::fmt::Debug {
    fn repo_name(&self) -> &str;
    fn repo_owner(&self) -> &str;
    fn repo_host(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub host: String,
}

impl Repository for Repo { /* ... */ }

impl Repo {
    pub fn new(owner: &str, name: &str) -> Self;
    pub fn new_with_host(owner: &str, name: &str, host: &str) -> Self;
    pub fn from_full_name(nwo: &str) -> Result<Self>;
    pub fn from_full_name_with_host(nwo: &str, fallback_host: &str) -> Result<Self>;
    pub fn from_url(url: &url::Url) -> Result<Self>;
    pub fn full_name(&self) -> String;
    pub fn generate_url(&self, path: &str) -> String;
    pub fn format_remote_url(&self, protocol: &str) -> String;
    pub fn is_same(&self, other: &Self) -> bool;
}
```

### 2.6 GitClient

Maps Go's `git/client.go`.

```rust
#[derive(Debug)]
pub struct GitClient {
    pub gh_path: String,
    pub repo_dir: String,
    pub git_path: String,
    pub stderr: Box<dyn Write + Send>,
    pub stdin: Box<dyn Read + Send>,
    pub stdout: Box<dyn Write + Send>,
}

impl GitClient {
    pub async fn command(&self, args: &[&str]) -> Result<GitCommand>;
    pub async fn current_branch(&self) -> Result<String>;
    pub async fn remotes(&self) -> Result<Vec<GitRemote>>;
    pub async fn fetch(&self, remote: &str, refspec: &str) -> Result<()>;
    pub async fn push(&self, remote: &str, refspec: &str, opts: &PushOptions) -> Result<()>;
    pub async fn clone_repo(&self, url: &str, dest: &str, opts: &CloneOptions) -> Result<()>;
    pub async fn config_get(&self, key: &str) -> Result<Option<String>>;
    pub async fn config_set(&self, key: &str, value: &str) -> Result<()>;
    pub async fn log(&self, opts: &LogOptions) -> Result<Vec<Commit>>;
    pub async fn diff(&self, opts: &DiffOptions) -> Result<String>;
    pub async fn checkout(&self, branch: &str, opts: &CheckoutOptions) -> Result<()>;
    pub async fn merge(&self, branch: &str, opts: &MergeOptions) -> Result<()>;
}
```

### 2.7 GitHub API Client

Maps Go's `api/client.go` + `api/http_client.go`.

```rust
#[derive(Debug, Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
}

impl GitHubClient {
    /// Execute a raw GraphQL query.
    pub async fn graphql<T: DeserializeOwned>(
        &self,
        hostname: &str,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T>;

    /// Execute a named GraphQL query (maps Go's Query method).
    pub async fn query<T: DeserializeOwned>(
        &self,
        hostname: &str,
        name: &str,
        variables: serde_json::Value,
    ) -> Result<T>;

    /// Execute a GraphQL mutation.
    pub async fn mutate<T: DeserializeOwned>(
        &self,
        hostname: &str,
        name: &str,
        variables: serde_json::Value,
    ) -> Result<T>;

    /// Execute a REST request.
    pub async fn rest<T: DeserializeOwned>(
        &self,
        hostname: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<T>;

    /// Execute a REST request and return the next page URL from Link header.
    pub async fn rest_with_next<T: DeserializeOwned>(
        &self,
        hostname: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<(T, Option<String>)>;
}
```

### 2.8 Prompter Trait

Maps Go's `internal/prompter/prompter.go`.

```rust
pub trait Prompter: Send + Sync + std::fmt::Debug {
    fn select(&self, prompt: &str, default: &str, options: &[String]) -> Result<usize>;
    fn multi_select(&self, prompt: &str, defaults: &[String], options: &[String]) -> Result<Vec<usize>>;
    fn input(&self, prompt: &str, default: &str) -> Result<String>;
    fn password(&self, prompt: &str) -> Result<String>;
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;

    // gh-specific prompts
    fn auth_token(&self) -> Result<String>;
    fn confirm_deletion(&self, required_value: &str) -> Result<()>;
    fn input_hostname(&self) -> Result<String>;
    fn markdown_editor(&self, prompt: &str, default: &str, blank_allowed: bool) -> Result<String>;
}
```

### 2.9 Browser Trait

Maps Go's `internal/browser/browser.go`.

```rust
pub trait Browser: Send + Sync + std::fmt::Debug {
    fn browse(&self, url: &str) -> Result<()>;
}
```

### 2.10 Extension Traits

Maps Go's `pkg/extensions/extension.go`.

```rust
pub trait Extension: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &str;
    fn path(&self) -> &str;
    fn url(&self) -> &str;
    fn current_version(&self) -> &str;
    fn latest_version(&self) -> &str;
    fn is_pinned(&self) -> bool;
    fn update_available(&self) -> bool;
    fn is_binary(&self) -> bool;
    fn is_local(&self) -> bool;
    fn owner(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtTemplateType {
    Git,
    GoBinary,
    OtherBinary,
}

#[async_trait]
pub trait ExtensionManager: Send + Sync + std::fmt::Debug {
    fn list(&self) -> Vec<Box<dyn Extension>>;
    async fn install(&self, repo: &dyn Repository, pin: &str) -> Result<()>;
    fn install_local(&self, dir: &str) -> Result<()>;
    async fn upgrade(&self, name: &str, force: bool) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
    async fn dispatch(
        &self,
        args: &[String],
        stdin: &mut dyn Read,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
    ) -> Result<bool>;
    fn create(&self, name: &str, template_type: ExtTemplateType) -> Result<()>;
}
```

### 2.11 Exporter Trait

Maps Go's `pkg/cmdutil/json_flags.go` Exporter interface.

```rust
pub trait Exporter: Send + Sync + std::fmt::Debug {
    fn fields(&self) -> &[String];
    fn write(&self, ios: &IOStreams, data: &serde_json::Value) -> Result<()>;
}
```

### 2.12 Factory

Maps Go's `pkg/cmdutil/factory.go` + `pkg/cmd/factory/default.go`. Uses `OnceCell` for lazy initialization matching Go's closure pattern.

```rust
#[derive(Debug)]
pub struct Factory {
    pub app_version: String,
    pub executable_name: String,

    // Eagerly initialized
    pub iostreams: Arc<IOStreams>,
    pub git_client: Arc<GitClient>,
    pub browser: Arc<dyn Browser>,
    pub prompter: Arc<dyn Prompter>,
    pub extension_manager: Arc<dyn ExtensionManager>,

    // Lazily initialized (matches Go's closure pattern)
    config: OnceCell<Arc<dyn Config>>,
    http_client: OnceCell<GitHubClient>,
    plain_http_client: OnceCell<reqwest::Client>,
    remotes: OnceCell<Vec<Remote>>,
    base_repo: OnceCell<Box<dyn Repository>>,
    branch: OnceCell<String>,
}

impl Factory {
    pub fn new(app_version: &str) -> Self;
    pub fn config(&self) -> Result<Arc<dyn Config>>;
    pub fn http_client(&self) -> Result<&GitHubClient>;
    pub fn plain_http_client(&self) -> Result<&reqwest::Client>;
    pub async fn remotes(&self) -> Result<&[Remote]>;
    pub async fn base_repo(&self) -> Result<&dyn Repository>;
    pub async fn smart_base_repo(&self) -> Result<&dyn Repository>;
    pub async fn branch(&self) -> Result<&str>;
    pub fn executable(&self) -> &str;
}
```

## 3. Dependencies (Latest Stable Versions as of February 2026)

### Workspace Dependencies (`[workspace.dependencies]`)

#### Runtime

| Crate | Version | Purpose | Maps To (Go) |
|-------|---------|---------|---------------|
| `tokio` | `~1.49` | Async runtime | goroutines |
| `clap` | `~4.5` | CLI framework (derive) | cobra |
| `reqwest` | `~0.13` | HTTP client (rustls) | net/http + go-gh |
| `rustls` | `~0.23` | TLS (with aws-lc-rs) | crypto/tls |
| `serde` | `~1.0` | Serialization framework | encoding/json |
| `serde_json` | `~1.0` | JSON | encoding/json |
| `serde_yaml_ng` | `~0.10` | YAML config files | go-yaml |
| `thiserror` | `~2.0` | Library error types | errors |
| `anyhow` | `~1.0` | Application errors | fmt.Errorf |
| `tracing` | `~0.1` | Structured logging | log |
| `tracing-subscriber` | `~0.3` | Log output (env-filter, json) | log |
| `dialoguer` | `~0.12` | Interactive prompts | survey/v2 + go-gh/prompter |
| `console` | `~0.16` | Terminal styling/colors | go-gh/term + go-colorable |
| `indicatif` | `~0.18` | Progress bars/spinners | briandowns/spinner |
| `comfy-table` | `~7.2` | Table output | go-gh/tableprinter |
| `crossterm` | `~0.29` | Terminal manipulation | go-gh/term + go-isatty |
| `termimad` | `~0.34` | Markdown rendering | charmbracelet/glamour |
| `secrecy` | `~0.10` | Secret handling | (no Go equivalent) |
| `keyring` | `~3.6` | OS keyring for tokens | internal/keyring |
| `subtle` | `~2.6` | Constant-time comparison | crypto/subtle |
| `url` | `~2.5` | URL parsing | net/url |
| `open` | `~5.3` | Open URLs in browser | go-gh/browser |
| `jaq-core` | `~2.2` | jq filtering (core) | go-gh/jq |
| `jaq-std` | `~2.1` | jq filtering (stdlib) | go-gh/jq |
| `chrono` | `~0.4` | Date/time | time |
| `regex` | `~1.11` | Regular expressions | regexp |
| `dirs` | `~6.0` | Platform directories | os.UserHomeDir |
| `shell-words` | `~1.1` | Shell lexing | google/shlex |
| `dashmap` | `~6.1` | Concurrent HashMap | sync.Map |
| `arc-swap` | `~1.8` | Lock-free shared data | sync/atomic |
| `typed-builder` | `~0.23` | Builder pattern | (no Go equivalent) |
| `smallvec` | `~1.15` | Stack-allocated small vecs | (no Go equivalent) |
| `once_cell` | `~1.21` | Lazy initialization | sync.Once |
| `validator` | `~0.20` | Input validation | (manual in Go) |
| `dotenvy` | `~0.15` | .env files for testing | (manual in Go) |
| `async-trait` | `~0.1` | Object-safe async traits | (no Go equivalent) |

#### Testing

| Crate | Version | Purpose |
|-------|---------|---------|
| `rstest` | `~0.26` | Parameterized tests |
| `proptest` | `~1.10` | Property-based testing |
| `mockall` | `~0.14` | Trait mocking |
| `wiremock` | `~0.6` | HTTP mocking |
| `tempfile` | `~3.24` | Temporary files/directories |
| `assert_cmd` | `~2.0` | CLI integration testing |
| `predicates` | `~3.1` | Test assertions |
| `insta` | `~1.42` | Snapshot testing |

### Dependency Rationale

| Choice | Rationale |
|--------|-----------|
| `reqwest` + `rustls` + `aws-lc-rs` | CLAUDE.md mandates `rustls` with `aws-lc-rs` crypto backend. Never use `native-tls`. |
| `serde_yaml_ng` | `serde_yaml` is deprecated/archived. `serde_yaml_ng` is the maintained community fork by the original contributors. |
| `clap` derive | Best match for cobra's subcommand model. Derive macro provides compile-time validation. |
| `dialoguer` + `console` | Pure Rust. Maps to Go's `survey/v2` + `go-gh/pkg/prompter`. |
| `comfy-table` | Maps to Go's `go-gh/pkg/tableprinter`. Pure Rust, auto-wrapping. |
| `keyring` v3.6 | Cross-platform credential storage (macOS Keychain, Windows Credential Manager, Linux Secret Service). Maps to Go's `internal/keyring`. |
| `jaq-core` + `jaq-std` | Pure Rust jq implementation. Maps to Go's `go-gh/v2/pkg/jq`. Faster than jq on most benchmarks. |
| `termimad` | Terminal markdown rendering. Maps to Go's `charmbracelet/glamour`. |
| `open` | Cross-platform URL/file opener. Maps to Go's `go-gh/v2/pkg/browser`. |
| `crossterm` | Terminal manipulation (TTY detection, colors, terminal size). Pure Rust, cross-platform. |

## 4. Command Registration Pattern (clap)

### 4.1 Directory Structure

Each command group lives in its own module under `ghc-cli/src/commands/`. This maps 1:1 to Go's `pkg/cmd/<group>/` layout.

```
ghc-cli/src/commands/
  mod.rs                  # Re-exports all command groups
  repo/
    mod.rs                # RepoCommand enum with subcommands
    list.rs               # `gh repo list`
    create.rs             # `gh repo create`
    clone.rs              # `gh repo clone`
    fork.rs               # `gh repo fork`
    view.rs               # `gh repo view`
    edit.rs
    delete.rs
    rename.rs
    archive.rs
    unarchive.rs
    sync.rs
    set_default.rs
    deploy_key/
      mod.rs
      add.rs
      list.rs
      delete.rs
    autolink/
      mod.rs
      create.rs
      list.rs
      view.rs
    credits.rs
    garden.rs
    gitignore/
      mod.rs
      list.rs
      view.rs
    license/
      mod.rs
      list.rs
      view.rs
  issue/
    mod.rs
    list.rs
    create.rs
    view.rs
    close.rs
    reopen.rs
    edit.rs
    delete.rs
    comment.rs
    lock.rs
    pin.rs
    unpin.rs
    transfer.rs
    status.rs
    develop.rs
    shared.rs             # Shared types/helpers for issue commands
  pr/
    mod.rs
    list.rs
    create.rs
    view.rs
    close.rs
    reopen.rs
    merge.rs
    checkout.rs
    diff.rs
    checks.rs
    ready.rs
    review.rs
    comment.rs
    status.rs
    edit.rs
    revert.rs
    update_branch.rs
    shared.rs
  (... all 35 command groups follow same pattern)
```

### 4.2 Command Definition Pattern

Each command uses clap derive macros. The pattern follows Go's `NewCmd<Name>(f *cmdutil.Factory, runF func) *cobra.Command`:

```rust
use clap::Args;
use anyhow::{Context, Result};

/// List issues in a repository
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Filter by assignee
    #[arg(short = 'a', long)]
    assignee: Option<String>,

    /// Filter by label (can be repeated)
    #[arg(short = 'l', long)]
    label: Vec<String>,

    /// Filter by state
    #[arg(short = 's', long, default_value = "open", value_parser = ["open", "closed", "all"])]
    state: String,

    /// Maximum number of results
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Open in web browser
    #[arg(short = 'w', long)]
    web: bool,

    /// Output JSON with specified fields
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a template
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ListArgs {
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        let client = factory.http_client()
            .context("failed to create HTTP client")?;
        let repo = factory.base_repo().await
            .context("could not determine base repository")?;
        // ... implementation
    }
}
```

### 4.3 Root Command Assembly

```rust
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "gh",
    about = "GitHub CLI",
    long_about = "Work seamlessly with GitHub from the command line.",
    version,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    // Core commands
    #[command(subcommand, about = "Manage repositories")]
    Repo(commands::repo::RepoCommand),
    #[command(subcommand, about = "Manage issues")]
    Issue(commands::issue::IssueCommand),
    #[command(subcommand, about = "Manage pull requests")]
    Pr(commands::pr::PrCommand),

    // Actions commands
    #[command(subcommand, about = "View details about workflow runs")]
    Run(commands::run::RunCommand),
    #[command(subcommand, about = "View details about GitHub Actions workflows")]
    Workflow(commands::workflow::WorkflowCommand),

    // ... all 35 command groups
}
```

### 4.4 Command Group Categorization

Matching Go's cobra.Group:

| Group ID | Title | Commands |
|----------|-------|----------|
| `core` | Core commands | `repo`, `issue`, `pr`, `gist`, `org`, `project` |
| `actions` | GitHub Actions commands | `run`, `workflow`, `cache`, `attestation` |
| `extension` | Extension commands | (dynamically registered) |
| `alias` | Alias commands | (dynamically registered per-parent) |

## 5. Error Handling Strategy

Per CLAUDE.md: `thiserror` for library crates, `anyhow` for application code.

### 5.1 Library Error Types

Each library crate defines domain-specific errors with `thiserror`:

```rust
// ghc-core/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("")]
    Silent,

    #[error("")]
    Cancel,

    #[error("")]
    Pending,

    #[error("{message}")]
    Auth { message: String },

    #[error("{message}")]
    Flag { message: String },

    #[error("{message}")]
    NoResults { message: String },
}

// ghc-api/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        scopes_suggestion: Option<String>,
        #[source]
        source: Option<reqwest::Error>,
    },

    #[error("GraphQL errors: {0:?}")]
    GraphQL(Vec<GraphQLErrorItem>),

    #[error("authentication required for {hostname}")]
    AuthRequired { hostname: String },

    #[error("rate limit exceeded, resets at {reset_at}")]
    RateLimit { reset_at: String },

    #[error("scope {scope} required; run: gh auth refresh -h {hostname} -s {scope}")]
    MissingScope { scope: String, hostname: String },

    #[error(transparent)]
    Request(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

// ghc-git/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git: {message}")]
    Command { message: String, exit_code: Option<i32> },

    #[error("not a git repository")]
    NotARepo,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ghc-config/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Read(#[source] std::io::Error),

    #[error("failed to write config: {0}")]
    Write(#[source] std::io::Error),

    #[error("failed to parse config: {0}")]
    Parse(#[source] serde_yaml_ng::Error),

    #[error("keyring error: {0}")]
    Keyring(String),

    #[error("migration failed: {0}")]
    Migration(String),
}
```

### 5.2 Application Error Handling

Command handlers use `anyhow::Result` with `.context()`:

```rust
pub async fn run(factory: &Factory, args: &ListArgs) -> Result<()> {
    let client = factory.http_client()
        .context("failed to create HTTP client")?;
    let repo = factory.base_repo().await
        .context("could not determine base repository")?;
    // ...
}
```

### 5.3 Exit Codes

Maps Go's `internal/ghcmd/cmd.go` exit codes:

```rust
#[repr(i32)]
pub enum ExitCode {
    Ok = 0,
    Error = 1,
    Cancel = 2,
    Auth = 4,
    Pending = 8,
}
```

## 6. Authentication Flow Design

### 6.1 Token Resolution Order

Matching Go's `AuthConfig.ActiveToken()`:

1. Environment variables (`GH_TOKEN`, `GITHUB_TOKEN`, `GH_ENTERPRISE_TOKEN`)
2. YAML config file (`hosts.<hostname>.oauth_token`)
3. System keyring (via `keyring` crate, keyed by `"gh:<hostname>"`)

### 6.2 OAuth Device Flow

Maps Go's `internal/authflow/flow.go`:

```rust
pub struct OAuthFlow {
    client_id: &'static str,
    client_secret: secrecy::SecretString,
    scopes: Vec<String>,
    hostname: String,
    http_client: reqwest::Client,
}

impl OAuthFlow {
    /// Initiates OAuth device flow:
    /// 1. POST to /login/device/code to get device_code + user_code
    /// 2. Display user_code, optionally copy to clipboard
    /// 3. Prompt user to open browser at verification URL
    /// 4. Poll /login/oauth/access_token until authorized
    /// 5. Fetch username via API call to /user
    /// 6. Return (access_token, username)
    pub async fn run(
        &self,
        ios: &IOStreams,
        browser: &dyn Browser,
        interactive: bool,
        copy_to_clipboard: bool,
    ) -> Result<(secrecy::SecretString, String)>;
}
```

### 6.3 Minimum Scopes

```rust
const MINIMUM_SCOPES: &[&str] = &["repo", "read:org", "gist"];
```

### 6.4 Multi-Account Support

Matching Go's user switching:
- Each host can have multiple users stored under `hosts.<hostname>.users.<username>`
- Tokens stored in keyring keyed as `"gh:<hostname>"` (active) and `"gh:<hostname>/<username>"` (per-user)
- `switch_user` moves the target user's token into the active slot

## 7. HTTP Client Architecture

### 7.1 Client Construction

Per CLAUDE.md: rustls with aws-lc-rs crypto backend.

```rust
pub struct HttpClientOptions {
    pub app_version: String,
    pub config: Option<Arc<dyn AuthConfig>>,
    pub enable_cache: bool,
    pub cache_ttl: std::time::Duration,
    pub log_writer: Option<Box<dyn Write + Send>>,
    pub log_colorize: bool,
    pub log_verbose: bool,
    pub skip_default_headers: bool,
}

pub fn build_http_client(opts: HttpClientOptions) -> Result<reqwest::Client> {
    let builder = reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .user_agent(format!("GitHub CLI {}", opts.app_version))
        .gzip(true)
        .brotli(true);

    builder.build().map_err(Into::into)
}
```

### 7.2 Auth Middleware

Maps Go's `api.AddAuthTokenHeader` round tripper:

```rust
/// HTTP client wrapper that injects auth tokens per-host.
#[derive(Debug, Clone)]
pub struct AuthHttpClient {
    inner: reqwest::Client,
    token_getter: Arc<dyn Fn(&str) -> Option<secrecy::SecretString> + Send + Sync>,
    sso_header: Arc<std::sync::Mutex<Option<String>>>,
}

impl AuthHttpClient {
    pub async fn execute(&self, mut request: reqwest::Request) -> Result<reqwest::Response> {
        // Inject auth token per-host (skip on redirect to different host)
        let hostname = request.url().host_str().unwrap_or_default();
        if request.headers().get("Authorization").is_none() {
            if let Some(token) = (self.token_getter)(hostname) {
                use secrecy::ExposeSecret;
                request.headers_mut().insert(
                    "Authorization",
                    format!("token {}", token.expose_secret()).parse()?,
                );
            }
        }

        let response = self.inner.execute(request).await?;

        // Extract SSO header for SAML enforcement
        if let Some(sso) = response.headers().get("X-GitHub-SSO") {
            if let Ok(val) = sso.to_str() {
                *self.sso_header.lock().unwrap() = Some(val.to_string());
            }
        }

        Ok(response)
    }
}
```

### 7.3 GraphQL Features Header

Maps Go's `graphqlFeatures = "merge_queue"`:

```rust
const GRAPHQL_FEATURES_HEADER: &str = "GraphQL-Features";
const GRAPHQL_FEATURES_VALUE: &str = "merge_queue";
```

## 8. Output Formatting

### 8.1 Table Printing

Maps Go's `internal/tableprinter/table_printer.go`:

```rust
pub struct TablePrinter {
    table: comfy_table::Table,
    is_tty: bool,
    color_scheme: ColorScheme,
    current_row: Vec<comfy_table::Cell>,
}

impl TablePrinter {
    pub fn new(ios: &IOStreams, headers: &[&str]) -> Self;
    pub fn add_field(&mut self, value: &str);
    pub fn add_field_with_color(&mut self, value: &str, color_fn: impl Fn(&str) -> String);
    pub fn add_time_field(&mut self, now: chrono::DateTime<chrono::Utc>, t: chrono::DateTime<chrono::Utc>);
    pub fn end_row(&mut self);
    pub fn render(&self, writer: &mut dyn Write) -> Result<()>;
    pub fn is_tty(&self) -> bool;
}
```

### 8.2 JSON/jq/template Export

Maps Go's `pkg/cmdutil/json_flags.go`:

```rust
pub struct JsonExporter {
    fields: Vec<String>,
    filter: Option<String>,   // jq expression via jaq-core
    template: Option<String>, // Go template syntax
}

impl Exporter for JsonExporter {
    fn fields(&self) -> &[String] { &self.fields }

    fn write(&self, ios: &IOStreams, data: &serde_json::Value) -> Result<()> {
        let filtered = self.select_fields(data);
        let json_bytes = serde_json::to_vec_pretty(&filtered)?;

        if let Some(ref jq_expr) = self.filter {
            let output = jaq_filter(&json_bytes, jq_expr)?;
            ios.write_stdout(&output)?;
        } else if let Some(ref template) = self.template {
            self.render_template(ios, &filtered, template)?;
        } else if ios.color_enabled() {
            write_colorized_json(ios, &json_bytes)?;
        } else {
            ios.write_stdout(&json_bytes)?;
        }
        Ok(())
    }
}
```

### 8.3 Go Template Equivalent

Go's `--template` flag uses Go templates. We implement a compatible subset:

```rust
/// Template engine for formatting JSON output.
/// Supports a subset of Go template syntax:
/// - {{ .FieldName }} - field access
/// - {{ range .Items }}...{{ end }} - iteration
/// - {{ if .Field }}...{{ end }} - conditionals
/// - {{ tablerow }}...{{ end }} - table formatting
/// - {{ color "green" }}...{{ end }} - color formatting
/// - {{ timeago .Date }} - fuzzy time
/// - {{ truncate N .Text }} - text truncation
pub struct TemplateEngine {
    width: u16,
    color_enabled: bool,
}
```

### 8.4 Markdown Rendering

Maps Go's `pkg/markdown/markdown.go` (glamour):

```rust
pub fn render_markdown(text: &str, width: u16, theme: &TerminalTheme) -> Result<String> {
    // Use termimad for terminal markdown rendering
    // Respect GH_MDWIDTH env var (max 120 unless overridden)
}
```

## 9. Configuration System Design

### 9.1 File Layout

```
~/.config/gh/
  config.yml    # Main configuration (merged hosts)
~/.local/state/gh/
  state.yml     # Transient state (update check timestamps)
~/.local/share/gh/
  extensions/   # Installed extensions
```

### 9.2 YAML Schema

```yaml
version: "1"
git_protocol: https
editor: ""
prompt: enabled
prefer_editor_prompt: disabled
pager: ""
aliases:
  co: pr checkout
http_unix_socket: ""
browser: ""
color_labels: disabled
accessible_colors: disabled
accessible_prompter: disabled
spinner: enabled
hosts:
  github.com:
    user: username
    oauth_token: <token>   # Only if keyring unavailable
    git_protocol: ssh
    users:
      username:
        oauth_token: <token>
```

### 9.3 Configuration Precedence

1. CLI flags
2. Environment variables (`GH_TOKEN`, `GH_HOST`, `GH_PAGER`, etc.)
3. Host-scoped config values (`hosts.<hostname>.<key>`)
4. Global config values (`<key>`)
5. Built-in defaults

### 9.4 Config Options

Maps Go's `internal/config/config.go` Options:

| Key | Default | Allowed Values | Description |
|-----|---------|---------------|-------------|
| `git_protocol` | `https` | `https`, `ssh` | Protocol for git operations |
| `editor` | `""` | any | Text editor |
| `prompt` | `enabled` | `enabled`, `disabled` | Interactive prompting |
| `prefer_editor_prompt` | `disabled` | `enabled`, `disabled` | Editor-based prompts |
| `pager` | `""` | any | Terminal pager program |
| `http_unix_socket` | `""` | any | Unix socket for HTTP |
| `browser` | `""` | any | Web browser |
| `color_labels` | `disabled` | `enabled`, `disabled` | RGB hex color labels |
| `accessible_colors` | `disabled` | `enabled`, `disabled` | 4-bit accessible colors |
| `accessible_prompter` | `disabled` | `enabled`, `disabled` | Accessible prompter |
| `spinner` | `enabled` | `enabled`, `disabled` | Animated spinner |

## 10. Git Integration Design

### 10.1 Architecture

Git client wraps the system `git` binary (same approach as Go CLI). Does NOT use `git2`/`libgit2` because:
- Go CLI shells out to `git` for all operations
- Ensures compatibility with user's git config, hooks, credential helpers
- `git2` has behavioral differences from `git` in edge cases

### 10.2 Remote Resolution

Maps Go's `context/remote.go`:

```rust
/// Remote sorted by priority: upstream > github > origin > other
pub fn sort_remotes(remotes: &mut [Remote]) {
    remotes.sort_by(|a, b| {
        remote_sort_score(&b.name).cmp(&remote_sort_score(&a.name))
    });
}

fn remote_sort_score(name: &str) -> u8 {
    match name.to_lowercase().as_str() {
        "upstream" => 3,
        "github" => 2,
        "origin" => 1,
        _ => 0,
    }
}

/// Translates git remotes to GitHub repo references,
/// filtering to only authenticated hosts.
pub fn translate_remotes(
    git_remotes: &[GitRemote],
    auth_hosts: &[String],
) -> Vec<Remote>;
```

### 10.3 Smart Base Repo Resolution

Maps Go's `factory.SmartBaseRepoFunc`:

1. Check if any remote has `gh-resolved` git config set to `base` or `owner/repo`
2. If non-interactive, return first remote
3. Query GitHub API to resolve repository network (parent repos)
4. If one result, return it as base
5. If multiple results, tell user to run `gh repo set-default`

## 11. Extension System Design

### 11.1 Extension Types

- **Git extensions**: Cloned repos with `gh-<name>` executable
- **Binary extensions**: Downloaded platform-specific binaries from GitHub releases
- **Local extensions**: Symlinked from local directory

### 11.2 Extension Directory

```
~/.local/share/gh/extensions/
  gh-copilot/
  gh-dash/
  ...
```

### 11.3 Extension Manager

```rust
pub struct ExtManager {
    extensions_dir: PathBuf,
    ios: Arc<IOStreams>,
    git_client: Arc<GitClient>,
    config: Option<Arc<dyn Config>>,
    http_client: Option<reqwest::Client>,
    dry_run: bool,
}
```

## 12. Go Package to Rust Module Mapping

| Go Package | Rust Crate/Module | Notes |
|------------|-------------------|-------|
| `cmd/gh/main.go` | `ghc/src/main.rs` | Binary entry point |
| `internal/ghcmd/` | `ghc/src/main.rs` | Main function, exit code handling |
| `pkg/cmd/root/` | `ghc-cli/src/root.rs` | Root command, help topics, alias/extension registration |
| `pkg/cmd/factory/` | `ghc-cli/src/factory.rs` | Factory struct with lazy deps |
| `pkg/cmdutil/factory.go` | `ghc-cli/src/factory.rs` | Factory type definition |
| `pkg/cmdutil/errors.go` | `ghc-core/src/error.rs` | Sentinel errors, FlagError |
| `pkg/cmdutil/json_flags.go` | `ghc-term/src/export.rs` | JSON/jq/template export |
| `pkg/cmdutil/auth_check.go` | `ghc-cli/src/auth_check.rs` | Auth check middleware |
| `pkg/cmdutil/flags.go` | `ghc-cli/src/flags.rs` | Shared flag helpers |
| `pkg/cmdutil/repo_override.go` | `ghc-cli/src/factory.rs` | GH_REPO override |
| `pkg/iostreams/` | `ghc-term/src/iostreams.rs` | IOStreams, TTY, pager, spinner |
| `pkg/iostreams/color.go` | `ghc-term/src/color.rs` | ColorScheme |
| `api/client.go` | `ghc-api/src/client.rs` | GraphQL + REST client |
| `api/http_client.go` | `ghc-api/src/http.rs` | HTTP client construction, middleware |
| `api/queries_*.go` | `ghc-api/src/queries/*.rs` | Domain-specific API queries |
| `git/client.go` | `ghc-git/src/client.rs` | Git command wrapper |
| `git/url.go` | `ghc-git/src/url.rs` | Git URL parsing |
| `context/remote.go` | `ghc-core/src/remote.rs` | Remote types, sorting |
| `context/context.go` | `ghc-git/src/remote.rs` | Remote translation, resolution |
| `internal/config/config.go` | `ghc-config/src/config.rs` | YAML config implementation |
| `internal/config/auth_config` (inline) | `ghc-config/src/auth_config.rs` | Auth config + keyring |
| `internal/gh/gh.go` | `ghc-core/src/config.rs` | Config/Auth/Alias trait definitions |
| `internal/ghrepo/repo.go` | `ghc-core/src/repo.rs` | Repository types |
| `internal/ghinstance/` | `ghc-core/src/instance.rs` | Hostname normalization |
| `internal/authflow/` | `ghc-auth/src/oauth.rs` | OAuth device flow |
| `internal/browser/` | `ghc-term/src/browser.rs` | Browser trait + open impl |
| `internal/prompter/` | `ghc-term/src/prompter.rs` | Prompter trait + impls |
| `internal/tableprinter/` | `ghc-term/src/table.rs` | Table printer |
| `internal/text/` | `ghc-term/src/text.rs` | Text utilities |
| `internal/keyring/` | `ghc-config/src/keyring.rs` | Credential storage |
| `internal/featuredetection/` | `ghc-api/src/feature.rs` | API feature detection |
| `internal/update/` | `ghc-cli/src/update.rs` | Update checker |
| `pkg/search/` | `ghc-search/src/` | Search query builder |
| `pkg/extensions/` | `ghc-ext/src/` | Extension types + manager |
| `pkg/markdown/` | `ghc-term/src/markdown.rs` | Markdown rendering |
| `pkg/jsoncolor/` | `ghc-term/src/json_color.rs` | Colorized JSON |
| `pkg/ssh/` | `ghc-cli/src/commands/ssh_key/` | SSH key operations |
| `pkg/set/` | N/A (use `std::collections::HashSet`) | |
| `pkg/option/` | N/A (use `std::option::Option`) | |
| `pkg/surveyext/` | `ghc-term/src/prompter.rs` | Editor-based prompts |
| `pkg/cmd/<group>/` | `ghc-cli/src/commands/<group>/` | 1:1 mapping for all 35 groups |

## 13. Environment Variables

All environment variables from the Go CLI must be preserved:

| Variable | Purpose |
|----------|---------|
| `GH_TOKEN` / `GITHUB_TOKEN` | Auth token (highest priority) |
| `GH_ENTERPRISE_TOKEN` / `GITHUB_ENTERPRISE_TOKEN` | Enterprise auth token |
| `GH_HOST` | Override default hostname |
| `GH_REPO` | Override repository (OWNER/REPO) |
| `GH_PATH` | Override executable path |
| `GH_PAGER` | Override pager (priority over config) |
| `GH_PROMPT_DISABLED` | Disable interactive prompts |
| `GH_FORCE_TTY` | Force TTY mode for piped output |
| `GH_NO_UPDATE_NOTIFIER` | Disable update checks |
| `GH_CONFIG_DIR` | Override config directory |
| `GH_DEBUG` | Enable debug logging (`api` for verbose HTTP) |
| `GH_SPINNER_DISABLED` | Disable animated spinner |
| `GH_COLOR_LABELS` | Enable/disable color labels |
| `GH_ACCESSIBLE_PROMPTER` | Enable accessible prompter |
| `GH_MDWIDTH` | Override markdown rendering width |
| `GLAMOUR_STYLE` | Override markdown theme |
| `PAGER` | System pager (lowest priority) |
| `VISUAL` / `EDITOR` | Text editor |
| `BROWSER` | Default web browser |
| `NO_COLOR` | Disable all color output |

## 14. Build and Toolchain Configuration

### 14.1 `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.86.0"
components = ["rustfmt", "clippy"]
```

### 14.2 Workspace `Cargo.toml` Lints

```toml
[workspace.lints.rust]
rust_2024_compatibility = "warn"
missing_docs = "warn"
missing_debug_implementations = "warn"
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
```

### 14.3 Makefile Targets

```makefile
.PHONY: build test fmt lint audit deny check release

build:
	cargo build

test:
	cargo test

fmt:
	cargo +nightly fmt

lint:
	cargo clippy -- -D warnings -W clippy::pedantic

audit:
	cargo audit

deny:
	cargo deny check

check: fmt lint test build

release:
	cargo build --release
```

## 15. 35 Command Groups (Full Inventory)

| # | Group | Subcommands | API Type | Auth Required |
|---|-------|-------------|----------|---------------|
| 1 | `auth` | login, logout, status, refresh, token, switch, setup-git, git-credential | OAuth/REST | No |
| 2 | `repo` | clone, create, fork, view, list, edit, delete, rename, archive, unarchive, sync, set-default, deploy-key, autolink, credits, garden, gitignore, license | GraphQL+REST | Yes |
| 3 | `issue` | create, list, view, edit, close, reopen, delete, comment, lock, pin, unpin, transfer, status, develop | GraphQL | Yes |
| 4 | `pr` | create, list, view, edit, close, reopen, merge, checkout, diff, checks, ready, review, comment, status, revert, update-branch | GraphQL+REST | Yes |
| 5 | `release` | create, list, view, edit, delete, download, upload, delete-asset | REST | Yes |
| 6 | `gist` | create, list, view, edit, delete, clone, rename | REST | Yes |
| 7 | `workflow` | list, view, enable, disable, run | REST | Yes |
| 8 | `run` | list, view, rerun, download, watch, cancel, delete | REST | Yes |
| 9 | `cache` | list, delete | REST | Yes |
| 10 | `search` | repos, issues, prs, commits, code | REST | Yes |
| 11 | `project` | create, list, view, edit, close, delete, copy, link, unlink, mark-template, field-create, field-list, field-delete, item-add, item-create, item-edit, item-list, item-archive, item-delete | GraphQL | Yes |
| 12 | `secret` | set, list, delete | REST | Yes |
| 13 | `variable` | set, list, get, delete | REST | Yes |
| 14 | `label` | create, list, edit, delete | REST | Yes |
| 15 | `config` | get, set, list, clear-cache | Local | No |
| 16 | `alias` | set, list, delete, import | Local | No |
| 17 | `extension` | install, list, upgrade, remove, create, browse, exec, search | Mixed | No |
| 18 | `ssh-key` | add, list, delete | REST | Yes |
| 19 | `gpg-key` | add, list, delete | REST | Yes |
| 20 | `org` | list | GraphQL | Yes |
| 21 | `codespace` | create, list, view, edit, delete, ssh, ports, cp, logs, code, jupyter, stop, rebuild | REST | Yes |
| 22 | `ruleset` | list, view, check | GraphQL | Yes |
| 23 | `attestation` | verify, download | REST | Yes |
| 24 | `copilot` | (extension wrapper) | Extension | No |
| 25 | `completion` | (generates shell completions for bash, zsh, fish, powershell) | Local | No |
| 26 | `version` | (standalone) | Local | No |
| 27 | `status` | (standalone, shows notifications/mentions/review-requests) | GraphQL | Yes |
| 28 | `browse` | (standalone, opens repo/issue/pr in browser) | Local+API | Mixed |
| 29 | `api` | (standalone, raw API requests) | REST/GraphQL | Yes |
| 30 | `actions` | (info page only) | None | No |
| 31 | `accessibility` | (info page only) | None | No |
| 32 | `preview` | (feature flags) | Local | No |
| 33 | `agent-task` | create, list, view | REST | Yes |
| 34 | `credits` | (standalone, shows repo contributors) | GraphQL | Yes |

## 16. Phase Implementation Order

### Phase 1: Foundation
1. Workspace setup with all crate stubs, `Cargo.toml` configs, `rust-toolchain.toml`
2. `ghc-core`: Error types, Repository/Repo, Config/AuthConfig/AliasConfig traits, Remote types, GhInstance
3. `ghc-term`: IOStreams (system + test), ColorScheme, text utilities
4. `ghc-config`: YamlConfig, keyring integration, defaults, migration

### Phase 2: API and Git
5. `ghc-api`: HTTP client (reqwest + rustls + aws-lc-rs), auth middleware, REST/GraphQL client
6. `ghc-git`: GitClient, remote parsing, URL translation
7. `ghc-auth`: OAuth device flow, token validation

### Phase 3: CLI Framework
8. `ghc-cli`: Factory, root command, auth check, flag helpers
9. `ghc-cli`: `auth` commands (login, logout, status, token, switch)
10. `ghc-cli`: `config` commands (get, set, list, clear-cache)
11. `ghc-cli`: `version`, `completion` commands

### Phase 4: Core Commands
12. `ghc-cli`: `repo` commands (list, create, clone, fork, view, edit, delete, sync, archive, rename, set-default, deploy-key, autolink, credits, garden, gitignore, license)
13. `ghc-cli`: `issue` commands (list, create, view, close, reopen, edit, delete, comment, transfer, pin, unpin, lock, status, develop)
14. `ghc-cli`: `pr` commands (list, create, view, checkout, close, reopen, merge, edit, comment, review, diff, checks, ready, status, revert, update-branch)

### Phase 5: Actions and Search
15. `ghc-cli`: `run` commands (list, view, watch, rerun, cancel, download, delete)
16. `ghc-cli`: `workflow` commands (list, view, run, enable, disable)
17. `ghc-search`: Query builder, `search` commands (repos, issues, prs, commits, code)
18. `ghc-cli`: `cache` commands (list, delete)

### Phase 6: Remaining Commands
19. `ghc-cli`: `gist`, `release`, `secret`, `variable`, `label`, `ssh-key`, `gpg-key`
20. `ghc-cli`: `alias`, `api`, `browse`, `status`, `completion`
21. `ghc-cli`: `codespace`, `project`, `org`, `ruleset`, `copilot`, `attestation`, `agent-task`
22. `ghc-ext`: Extension system, `extension` commands

### Phase 7: Polish
23. Shell completions (bash, zsh, fish, powershell) via `clap_complete`
24. Help topics (reference, formatting, mintty, environment, exit-codes)
25. Update checker (background task)
26. Alias registration + shell alias execution
27. Output formatting: table printer, JSON export, jq filtering, template engine, markdown rendering

## 17. Testing Strategy

### Unit Tests
- Every module includes `#[cfg(test)] mod tests`
- Test names use `test_should_` prefix
- Error cases tested explicitly with `assert!(matches!(...))`
- `rstest` for parameterized tests
- `mockall` for trait mocking (Config, AuthConfig, Prompter, Browser, ExtensionManager)

### Integration Tests
- `wiremock` for HTTP mocking (GitHub API responses)
- `tempfile` for temporary config/repo directories
- `assert_cmd` + `predicates` for end-to-end command tests

### Property-Based Tests
- `proptest` for URL parsing, config key validation, search query building, remote sorting

### Parity Tests
- Compare output of `gh` (Go) vs `ghc` (Rust) for identical inputs
- Automated test suite under `tests/parity/`

## 18. Glossary

| Term | Definition |
|------|-----------|
| **Factory** | Dependency injection container with lazy-initialized shared services |
| **IOStreams** | Abstraction over stdin/stdout/stderr with TTY, color, pager, and spinner |
| **Base Repo** | The resolved GitHub repository for the current working directory |
| **Smart Base Repo** | Base repo resolution using API to resolve fork networks |
| **Remotes** | Git remotes mapped to GitHub repositories, sorted by priority |
| **Exporter** | JSON/jq/template output formatter for `--json`/`--jq`/`--template` flags |
| **Extension** | Third-party CLI plugin installed as `gh-<name>` |
| **ConfigEntry** | Config value with source tracking (default vs user-provided) |
| **Device Flow** | OAuth device authorization grant for CLI authentication |
