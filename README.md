[![Build Status](https://github.com/tyrchen/ghc/workflows/build/badge.svg)](https://github.com/tyrchen/ghc/actions)

# ghc - GitHub CLI in Rust

A fast, native Rust implementation of the [GitHub CLI](https://cli.github.com/) (`gh`). Drop-in replacement with the same command-line interface you already know.

## Why ghc?

- **Fast** - Native binary compiled with Rust, no runtime overhead
- **Compatible** - Same commands, same flags, same output format as `gh`
- **Secure** - Built with `rustls` TLS, `secrecy` for token handling, keyring integration for secure token storage
- **Extensible** - Modular crate architecture for embedding in other Rust projects

## Installation

### Prerequisites

- Rust toolchain (stable, 2024 edition). See [rust-toolchain.toml](rust-toolchain.toml) for the pinned version.
- An existing `gh` configuration (ghc reads `~/.config/gh/config.yml` and `~/.config/gh/hosts.yml`)

### From source

```bash
cargo install --path crates/ghc
```

### Build from repository

```bash
git clone https://github.com/tyrchen/ghc.git
cd ghc
cargo build --release
# Binary at target/release/ghc
```

## Quick Start

ghc reads your existing `gh` configuration, so if you already have `gh` set up, just start using `ghc`:

```bash
# Authenticate (or reuse existing gh config)
ghc auth login

# List your repositories
ghc repo list

# View an issue
ghc issue view 42 -R owner/repo

# Create a pull request
ghc pr create -R owner/repo --title "My PR" --body "Description"

# Search across GitHub
ghc search repos "rust cli" --limit 5

# Run API queries directly
ghc api repos/owner/repo --jq '.name'
```

## Feature Parity

ghc implements **100% of gh's command groups** (25/25) and **98% of subcommands** (145/148).

### CLI-to-CLI Output Parity

Tested against `gh` v2.86.0 across 23 diff comparisons:

| Result | Count | Details |
|--------|-------|---------|
| Identical | 22 | Byte-identical output to gh |
| Minor diff | 1 | Label sort order |

### Command Coverage

| Metric | Coverage |
|--------|----------|
| Command groups | 25/25 (100%) |
| Subcommands | 145/148 (98%) |
| Flag coverage | ~85% |

### Known Gaps

- `--repo`/`-R` is currently required (gh auto-detects from git context)
- 12 commands have `-h` short flag conflicts (commands work, `--help` panics)
- Some search/codespace/attestation flags are not yet implemented

See [docs/feature-parity-report.md](docs/feature-parity-report.md) for the full parity analysis.

## Supported Commands

### Core Commands

| Command Group | Description | Key Operations |
|---------------|-------------|----------------|
| `repo` | Repository operations | `view`, `list`, `clone`, `create`, `edit`, `delete`, `fork`, `rename`, `archive` |
| `issue` | Issue management | `create`, `list`, `view`, `close`, `reopen`, `comment`, `edit`, `lock`, `unlock`, `status` |
| `pr` | Pull request workflows | `create`, `list`, `view`, `merge`, `close`, `comment`, `review`, `diff`, `checks`, `status` |
| `gist` | Gist management | `create`, `list`, `view`, `edit`, `delete`, `clone`, `rename` |
| `release` | Release management | `create`, `list`, `view`, `delete`, `upload`, `download`, `edit` |
| `label` | Label management | `create`, `list`, `edit`, `delete`, `clone` |
| `search` | GitHub search | `repos`, `issues`, `prs`, `commits`, `code` |
| `api` | Direct API access | REST + GraphQL, `--jq`, pagination |

### Configuration & Auth

| Command Group | Description |
|---------------|-------------|
| `auth` | Authentication (`login`, `logout`, `status`, `token`, `switch`, `refresh`, `setup-git`) |
| `config` | Configuration (`list`, `get`, `set`, `clear-cache`) |
| `alias` | Command aliases (`set`, `list`, `delete`, `import`) |
| `ssh-key` | SSH key management (`list`, `add`, `delete`) |
| `gpg-key` | GPG key management (`list`, `add`, `delete`) |

### CI/CD & Automation

| Command Group | Description |
|---------------|-------------|
| `run` | Workflow run management (`list`, `view`, `watch`, `rerun`, `cancel`, `download`) |
| `workflow` | Workflow management (`list`, `view`, `enable`, `disable`, `run`) |
| `cache` | GitHub Actions cache management (`list`, `delete`) |
| `secret` | Repository/org secrets (`set`, `list`, `delete`) |
| `variable` | Repository/org variables (`set`, `list`, `get`, `delete`) |

### Additional Commands

| Command Group | Description |
|---------------|-------------|
| `browse` | Open repository in browser |
| `status` | Dashboard of notifications, assigned issues, and review requests |
| `codespace` | Codespaces management |
| `project` | GitHub Projects (v2) |
| `org` | Organization operations |
| `ruleset` | Repository rulesets |
| `attestation` | Artifact attestations |
| `copilot` | GitHub Copilot integration |
| `extension` | Extension management |
| `completion` | Shell completion scripts |

## Output Formatting

ghc supports the same output formatting flags as `gh`:

```bash
# JSON output with field selection
ghc issue list -R owner/repo --json number,title,state

# jq filtering (powered by jaq)
ghc pr list -R owner/repo --json title --jq '.[].title'

# Go template formatting
ghc repo list --json name,url --template '{{range .}}{{.name}}: {{.url}}{{"\n"}}{{end}}'
```

## Architecture

ghc is structured as a Rust workspace with five crates:

```
ghc/
  crates/
    ghc/          # Binary entry point
    ghc-cmd/      # All CLI command implementations (25 groups, 376+ tests)
    ghc-core/     # Shared types: config, auth, IOStreams, JSON/jq/template
    ghc-api/      # HTTP client for GitHub REST & GraphQL APIs
    ghc-git/      # Git operations (clone, push, credential helpers, 108+ tests)
```

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing with derive macros |
| `reqwest` + `rustls` | HTTP client with pure-Rust TLS |
| `jaq-*` | Full jq expression support |
| `secrecy` | Secure token handling |
| `keyring` | OS keyring integration for secure token storage |
| `tokio` | Async runtime |
| `serde` / `serde_json` | Serialization |

## Development

### Build & Test

```bash
# Build
cargo build

# Run all tests
cargo test --all

# Run with stricter checks
make check  # fmt + clippy + unit tests

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Format (requires nightly)
cargo +nightly fmt --all

# Install locally
cargo install --path crates/ghc
```

### Makefile Targets

| Target | Description |
|--------|-------------|
| `make build` | Build the project |
| `make test` | Run all tests with nextest |
| `make test-unit` | Run unit tests for all crates |
| `make test-clippy` | Run clippy lints |
| `make test-fmt` | Check formatting |
| `make check` | Run fmt + clippy + unit tests |
| `make release` | Tag and release |

## Configuration

ghc uses the same configuration files as `gh`:

- `~/.config/gh/config.yml` - General settings
- `~/.config/gh/hosts.yml` - Authentication tokens

### Token Storage

By default, `ghc auth login` stores tokens in the OS keyring (macOS Keychain, Windows Credential Manager, Linux Secret Service). Use `--insecure-storage` to store tokens in the config file instead.

```bash
# Default: secure keyring storage
ghc auth login

# Fallback: config file storage
ghc auth login --insecure-storage
```

### Configuration Keys

All 11 configuration keys from gh are supported:

| Key | Default | Description |
|-----|---------|-------------|
| `git_protocol` | `https` | Protocol for git operations (`https`/`ssh`) |
| `editor` | | Text editor for authoring |
| `prompt` | `enabled` | Interactive prompting |
| `prefer_editor_prompt` | `disabled` | Editor-based prompting preference |
| `pager` | | Terminal pager |
| `http_unix_socket` | | Unix socket for HTTP |
| `browser` | | Web browser for URLs |
| `color_labels` | `disabled` | RGB color labels in truecolor terminals |
| `accessible_colors` | `disabled` | 4-bit accessible colors |
| `accessible_prompter` | `disabled` | Accessible prompts |
| `spinner` | `enabled` | Animated spinner indicator |

## Documentation

- [Feature Parity Report](docs/feature-parity-report.md) - Full CLI-to-CLI parity analysis
- [Architecture Review](docs/architecture-review.md) - Workspace architecture review
- [CLI Parity Test Report](docs/cli-parity-test-report.md) - Detailed test results

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2025 Tyr Chen
