# GHC Architecture Review

**Date**: 2026-02-06
**Reviewer**: Architect Agent
**Scope**: Full workspace (ghc, ghc-core, ghc-api, ghc-git, ghc-cmd)

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Crate Architecture](#crate-architecture)
3. [Critical Issues](#critical-issues)
4. [Major Issues](#major-issues)
5. [Minor Issues](#minor-issues)
6. [Suggestions](#suggestions)
7. [What Works Well](#what-works-well)

---

## Executive Summary

The ghc codebase is a well-structured Rust workspace rewriting the Go-based GitHub CLI (`gh`). The crate boundaries are clean, the dependency flow is correct (no cycles), and the overall architecture is sound. The code demonstrates good Rust idioms, comprehensive test coverage, and proper use of `thiserror`/`anyhow` separation between library and application layers.

However, there are several areas that need attention, primarily around:
- Token/secret handling not using `secrecy` crate despite it being a dependency
- Significant DRY violations in REST error handling and test utility code
- 27 functions suppressing `clippy::too_many_lines` indicating need for refactoring
- Custom base64 implementation when `base64` crate is a workspace dependency
- Missing `#[non_exhaustive]` on library types

---

## Crate Architecture

### Dependency Graph

```
ghc (binary)
  -> ghc-cmd (command implementations)
  -> ghc-api (HTTP/GraphQL client)
  -> ghc-core (core types, traits)
  -> ghc-git (git operations)

ghc-cmd -> ghc-api, ghc-core, ghc-git
ghc-api -> ghc-core
ghc-git -> ghc-core
ghc-core -> (no internal deps)
```

**Assessment**: The dependency flow is clean and acyclic. Each crate has a clear responsibility:
- `ghc-core`: Foundation types, traits, IO, config -- no network dependencies
- `ghc-api`: HTTP client, REST/GraphQL, authentication flows
- `ghc-git`: Git CLI wrapper, remote parsing
- `ghc-cmd`: Command implementations with CLI argument parsing
- `ghc`: Binary entrypoint, CLI routing

This is a well-designed layered architecture.

---

## Critical Issues

### C1. Token Stored as Plain `String` -- `secrecy` Crate Not Used

**Files**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/client.rs:29`, `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/config/file_config.rs:46`

CLAUDE.md explicitly states: *"Use `secrecy` crate for handling secrets in memory (prevents accidental logging/exposure)."* The `secrecy` crate is listed as a workspace dependency and is included in both `ghc-core` and `ghc-api` Cargo.toml files. However, tokens are stored as plain `Option<String>` everywhere:

```rust
// client.rs:29
pub struct Client {
    token: Option<String>,  // Should be Option<Secret<String>>
}

// file_config.rs:46
struct HostConfig {
    oauth_token: Option<String>,  // Should be Secret<String>
}
```

Tokens flow through the system as plain strings, defeating the purpose of having `secrecy` as a dependency. The `#[derive(Debug, Clone)]` on `Client` would print the token in debug output.

**Risk**: Tokens could leak into logs, debug output, or error messages. This directly contradicts CLAUDE.md's "Never log, print, or expose sensitive data" directive.

### C2. Production Code Contains `unwrap_or_else(|_| Regex::new(".").expect("valid regex"))`

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-cmd/src/repo/create.rs:1281`

```rust
fn normalize_repo_name(name: &str) -> String {
    let re = Regex::new(r"[^\w._-]+").unwrap_or_else(|_| Regex::new(".").expect("valid regex"));
```

CLAUDE.md states: *"Never use `unwrap()` or `expect()` in production code."* While the regex is a compile-time constant string and will never fail, this pattern still violates the project guidelines. The fallback itself uses `expect()`. Use `once_cell::sync::Lazy` or `std::sync::LazyLock` for compile-time regex.

### C3. `unwrap_or_default()` Used for Error Response Text

**Files**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/client.rs:126`, `:179`, `:216`, `:253`, `:272`, `:375`, `:459`, `:495`

Multiple occurrences of `resp.text().await.unwrap_or_default()` in the `Client` methods silently swallow errors when reading the response body. While this is not `unwrap()`, it masks potentially useful error information. The error text is then used in the `ApiError::Http` message field, meaning error messages could be empty strings with no indication of why.

---

## Major Issues

### M1. Massive DRY Violation: REST Error Handling Pattern Repeated 6+ Times

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/client.rs`

The same error-handling block is copy-pasted across `rest()`, `rest_text()`, `rest_with_next()`, `rest_bytes()`, `upload_asset()`, and `get_scopes()`:

```rust
if !status.is_success() {
    let text = resp.text().await.unwrap_or_default();
    let suggestion = generate_scopes_suggestion(
        status.as_u16(),
        headers.get("x-accepted-oauth-scopes").and_then(|v| v.to_str().ok()),
        headers.get("x-oauth-scopes").and_then(|v| v.to_str().ok()),
    );
    return Err(ApiError::Http {
        status: status.as_u16(),
        message: text,
        scopes_suggestion: suggestion,
        headers: extract_header_map(&headers),
    });
}
```

This pattern appears with minor variations approximately 6 times. It should be extracted into a single helper method like `fn check_response(resp: &Response) -> Result<(), ApiError>`.

### M2. 27 Functions Suppress `clippy::too_many_lines`

**Files**: Various in `ghc-cmd/src/`

Twenty-seven functions across the command implementations have `#[allow(clippy::too_many_lines)]`. CLAUDE.md states: *"Keep functions small and focused... function should not be more than 150 lines of code."* The sheer volume of suppressions indicates a systemic need to break down command `run()` methods into smaller units.

Notable offenders:
- `repo/create.rs` has 3 separate functions with this annotation
- `repo/list.rs` has 2 functions with this annotation
- `pr/create.rs`, `pr/view.rs`, `pr/list.rs`, `pr/edit.rs`, `pr/revert.rs`, `pr/checkout.rs`, `pr/status.rs`, `pr/checks.rs`, `pr/comment.rs` -- nearly every PR subcommand

### M3. Custom Base64 Implementation When `base64` Crate Is a Workspace Dependency

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/text.rs:147-227`

The codebase includes a hand-written base64 encoder and decoder (approximately 80 lines) in `text.rs`, despite `base64 = "0.22"` being declared in the workspace `Cargo.toml`. The custom implementation:
- Has not been audited for correctness edge cases
- Lacks the `URL_SAFE` alphabet variant
- Contains a potential issue: `u8::try_from((buf >> bits_collected) & 0xFF).unwrap_or(0)` -- uses `unwrap_or(0)` which could silently corrupt data

The `base64` crate should be used directly or the workspace dependency should be removed if intentionally not needed in `ghc-core`.

### M4. `EnvVarGuard` / `EnvGuard` Duplicated 3 Times

**Files**:
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/config/mod.rs:299`
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/config/file_config.rs:637`
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/cmdutil.rs:174`

The RAII environment variable guard pattern is copy-pasted in three different test modules with slightly different names (`EnvVarGuard` vs `EnvGuard`). This should be extracted into a shared test utility in `ghc-core`.

### M5. `PullRequest.state` and `Issue.state` Are Plain `String` Instead of Enums

**Files**:
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/queries/pr.rs:18`
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/queries/issue.rs:16`

CLAUDE.md states: *"Use enums for state machines. Prefer type-state pattern for compile-time state enforcement."* And: *"Use Rust's type system to make illegal states unrepresentable."*

PR states (`OPEN`, `CLOSED`, `MERGED`) and issue states (`OPEN`, `CLOSED`) are well-defined enum sets. Using `String` means:
- No compile-time validation of state values
- String comparisons in match statements are error-prone
- No exhaustiveness checking

### M6. `Mutex<Box<dyn Config>>` Pattern in Factory

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-cmd/src/factory.rs:25`

```rust
config: OnceLock<Mutex<Box<dyn Config>>>,
```

CLAUDE.md states to use `ArcSwap` for infrequently updated shared data and `DashMap` for concurrent access. The `Mutex<Box<dyn Config>>` requires callers to hold the lock for the entire duration of config reads, leading to `#[allow(clippy::await_holding_lock)]` annotations (e.g., `auth/login.rs:65`, `auth/status.rs:74`). Holding a mutex across `.await` points is a known anti-pattern that can cause deadlocks.

### M7. `IOStreams` Is Not `Send` Due to `Mutex<Option<Child>>`

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/iostreams.rs:90-117`

`IOStreams` contains `Mutex<Option<Child>>` for pager process management and `Arc<Mutex<OutputWriter>>` for output capture. The struct uses `std::sync::Mutex` (not `tokio::sync::Mutex`), which is fine for synchronous access. However, the struct cannot be safely moved between tasks without `Arc` wrapping. The factory passes `IOStreams` by reference through `&factory.io`, but this constrains concurrent command execution patterns.

### M8. `#[allow(clippy::await_holding_lock)]` Used in Login/Status

**Files**:
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-cmd/src/auth/login.rs:65`
- `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-cmd/src/auth/status.rs:74`

Holding a `std::sync::Mutex` across `.await` points can cause deadlocks if the same task re-enters the lock. The code works because it drops the lock before async operations in `login.rs:109`, but the allow annotation suppresses a legitimate warning. The pattern should be restructured to always acquire/release the lock around individual sync operations, or use a read-copy-update pattern.

---

## Minor Issues

### m1. Missing `#[non_exhaustive]` on Library Types

**Files**: Various structs in `ghc-core` and `ghc-api`

CLAUDE.md states: *"Make structs non-exhaustive with `#[non_exhaustive]` for library types to allow future field additions."*

The following library types lack `#[non_exhaustive]`:
- `ghc_core::repo::Repo`
- `ghc_core::errors::CoreError`, `ConfigError`
- `ghc_core::config::ConfigOption`
- `ghc_api::errors::ApiError`
- `ghc_api::client::RestPage`, `PageInfo`
- `ghc_api::queries::pr::PullRequest`
- `ghc_api::queries::issue::Issue`, `Actor`, `Label`, etc.

### m2. `Regex::new` Called on Every Invocation of `parse_link_next`

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/client.rs:581`

```rust
fn parse_link_next(headers: &HeaderMap) -> Option<String> {
    let link_header = headers.get("link")?.to_str().ok()?;
    let re = Regex::new(r#"<([^>]+)>;\s*rel="([^"]+)""#).ok()?;
```

The regex is compiled on every function call. Use `std::sync::LazyLock` (or `once_cell::sync::Lazy`) to compile once. Also, the `.ok()?` silently swallows regex compilation errors (which should never happen for a constant pattern).

### m3. `eprintln!("Error: {e:#}")` in `main.rs` Instead of IOStreams

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc/src/main.rs:155`

CLAUDE.md states: *"Use `tracing` for structured logging and diagnostics. Never use `println!` or `dbg!` in production code."*

The main error handler uses `eprintln!` directly. While this is in the binary crate (not a library), using `tracing::error!` would enable structured logging and be consistent with the project guidelines. The `println!()` on line 163 has the same issue.

### m4. `#[allow(dead_code)]` on `PENDING` Exit Code

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc/src/main.rs:16-17`

```rust
#[allow(dead_code)]
pub const PENDING: i32 = 8;
```

CLAUDE.md states: *"Do not suppress dead code, remove them."* Either wire up `PendingError` handling in the match statement or remove the constant.

### m5. `struct_excessive_bools` Suppressed on 10+ Structs

**Files**: Various command argument structs across `ghc-cmd`

Multiple `#[allow(clippy::struct_excessive_bools)]` annotations on command argument structs (`LoginArgs`, `BrowseArgs`, `CreateArgs`, `ForkArgs`, etc.). While these mirror the Go CLI's flag structure, the pattern could be improved by grouping related booleans into enums. For example, `--web` vs `--with-token` in `LoginArgs` are mutually exclusive and could be an enum variant.

### m6. `FileConfig::write()` Uses `unwrap_or` on `parent()`

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-core/src/config/file_config.rs:231`

```rust
let dir = self.config_path.parent().unwrap_or(std::path::Path::new("."));
```

While `unwrap_or` is not `unwrap()`, falling back to "." when the config path has no parent is silently wrong behavior. The config path should always have a parent directory. This should return an error.

### m7. `current_login_with_token` Bypasses `graphql()` Method

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/client.rs:430-470`

The `current_login_with_token()` method manually constructs and sends a GraphQL request instead of using the existing `graphql()` method. It also uses a different response wrapper (`Wrapper { data: DataInner }`) compared to `current_login()` which uses the standard `graphql()` path. This is a DRY violation and the two methods could share logic via a helper that accepts an optional token override.

### m8. HTTP Client Does Not Configure TLS Backend

**File**: `/Users/tchen/projects/mycode/rust/ghc/crates/ghc-api/src/http.rs:42-44`

CLAUDE.md states: *"Use `rustls` with `aws-lc-rs` crypto backend for TLS. Never use `native-tls` or OpenSSL bindings."*

The `build_client()` function uses `reqwest::Client::builder().build()` without specifying the TLS backend. The `reqwest` dependency in `Cargo.toml` does not include `rustls-tls` or `native-tls` features, meaning it uses whatever default `reqwest` provides (which is typically `native-tls`). This needs to be explicitly configured.

---

## Suggestions

### S1. Extract Common Command Pattern into Shared Infrastructure

Many command `run()` methods follow the same pattern:
1. Parse repo from `--repo` flag
2. Create API client for the repo's host
3. Make API call
4. Format and display output

A `CommandContext` struct or a helper method on `Factory` could reduce boilerplate:
```rust
let ctx = factory.command_context(&self.repo)?;
// ctx.client, ctx.repo, ctx.ios already resolved
```

### S2. Use `typed-builder` for Structs with Many Fields

CLAUDE.md recommends `typed-builder` for structs with >5 fields. `PullRequest` (20+ fields), `IOStreams` (15+ fields), and several command argument structs could benefit from builders, though CLI argument structs get their builder behavior from `clap::Args`.

### S3. Add `#[instrument]` to Async Functions

CLAUDE.md recommends using `tracing::instrument` on async functions. Key async methods like `Client::rest()`, `Client::graphql()`, and command `run()` methods lack `#[instrument]` annotations, which would improve diagnostics and observability.

### S4. Consider `Cow<str>` for Hostname/Token Parameters

Several methods take `&str` and immediately `.to_string()` the value (e.g., `Client::new`, `Repo::with_host`). Using `Cow<str>` or `Into<String>` patterns could avoid unnecessary allocations when callers already own the string.

### S5. Pin Dependency Versions with `~` Prefix

CLAUDE.md states: *"Pin versions carefully. Use `~` for patch updates."* The workspace `Cargo.toml` uses bare version constraints (e.g., `anyhow = "1.0"`, `tokio = "1.49"`) which allow minor version bumps. Consider using `~` for stability.

### S6. Add `cargo-deny` and `cargo-audit` Configuration

CLAUDE.md mentions both tools but there are no `deny.toml` or audit configuration files in the repository. Adding these would enforce license policies and catch known vulnerabilities.

### S7. Improve Test Organization

The `ghc-cmd` test helpers (`TestHarness`) are well-designed. Consider adding:
- Integration tests in `tests/` directories
- Snapshot tests with `insta` for command output formatting
- More property-based tests for parsers and formatters

---

## What Works Well

1. **Clean crate boundaries**: Dependencies flow in one direction with no cycles. Each crate has a well-defined responsibility.

2. **Error handling architecture**: Proper use of `thiserror` in library crates (`ghc-core`, `ghc-api`) and `anyhow` in the application layer (`ghc-cmd`, `ghc`). Error types are well-structured with context.

3. **Test coverage**: Extensive unit tests in all crates. Good use of `rstest` for parameterized tests and `proptest` for property-based testing in `text.rs` and `repo.rs`.

4. **Test infrastructure**: `TestHarness` with `wiremock` integration provides a solid foundation for testing command implementations without network access. `IOStreams::test_with_output()` enables output capture.

5. **Configuration design**: The `Config` and `AuthConfig` traits with `FileConfig` and `MemoryConfig` implementations follow the Strategy pattern well, enabling clean test isolation.

6. **Workspace dependency management**: All shared dependencies are declared in the workspace `Cargo.toml`, preventing version drift.

7. **Idiomatic Rust**: Good use of `Result<T>`, `Option<T>`, iterator combinators, and pattern matching throughout. No `todo!()` or `unimplemented!()` macros found.

8. **Documentation**: Module-level `//!` docs on all library modules. `/// ` doc comments on public functions with `# Errors` sections.

9. **CLI structure**: Clean 1:1 mapping between Go CLI commands and Rust modules makes it easy to navigate and verify parity.

10. **Correct serde usage**: `#[serde(rename_all = "camelCase")]` consistently applied for GitHub API compatibility.
