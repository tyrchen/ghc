[![Build Status](https://github.com/tyrchen/ghc/workflows/build/badge.svg)](https://github.com/tyrchen/ghc/actions)

# ghc - GitHub CLI in Rust

A fast, native Rust implementation of the [GitHub CLI](https://cli.github.com/) (`gh`). Drop-in replacement with 92%+ feature parity and the same command-line interface you already know.

## Why ghc?

- **Fast** - Native binary compiled with Rust, no runtime overhead
- **Compatible** - Same commands, same flags, same output format as `gh`
- **Secure** - Built with `rustls` TLS, `secrecy` for token handling, no `unsafe` code
- **Extensible** - Modular crate architecture for embedding in other Rust projects

## Installation

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
# List your repositories
ghc repo list

# View an issue
ghc issue view 42 -R owner/repo

# Create a pull request
ghc pr create --title "My PR" --body "Description"

# Search across GitHub
ghc search repos "rust cli" --limit 5

# Run API queries directly
ghc api repos/owner/repo --jq '.name'
```

## Supported Commands

ghc implements the full gh CLI command set across 33 command groups:

### Core Commands (Full Parity)

| Command Group | Description | Key Operations |
|---------------|-------------|----------------|
| `repo` | Repository operations | `view`, `list`, `clone`, `create`, `edit`, `delete`, `fork`, `rename`, `archive` |
| `issue` | Issue management | `create`, `list`, `view`, `close`, `reopen`, `comment`, `edit`, `lock`, `unlock`, `status` |
| `pr` | Pull request workflows | `create`, `list`, `view`, `merge`, `close`, `comment`, `review`, `diff`, `checks`, `status` |
| `gist` | Gist management | `create`, `list`, `view`, `edit`, `delete`, `clone`, `rename` |
| `release` | Release management | `create`, `list`, `view`, `delete`, `upload`, `download` |
| `label` | Label management | `create`, `list`, `edit`, `delete` |
| `secret` | Repository/org secrets | `set`, `list`, `delete` |
| `variable` | Repository/org variables | `set`, `list`, `get`, `delete` |
| `workflow` | Workflow management | `list`, `view`, `enable`, `disable`, `run` |
| `run` | Workflow run management | `list`, `view`, `watch`, `rerun`, `cancel`, `download` |
| `search` | GitHub search | `repos`, `issues`, `prs`, `commits`, `code` |
| `api` | Direct API access | REST + GraphQL, `--jq`, `--template`, pagination |

### Configuration & Auth

| Command Group | Description |
|---------------|-------------|
| `auth` | Authentication (`login`, `logout`, `status`, `token`, `switch`) |
| `config` | Configuration (`list`, `get`, `set`, `clear-cache`) |
| `alias` | Command aliases (`set`, `list`, `delete`, `import`) |
| `ssh-key` | SSH key management (`list`, `add`, `delete`) |
| `gpg-key` | GPG key management (`list`, `add`, `delete`) |

### Additional Commands

| Command Group | Description |
|---------------|-------------|
| `browse` | Open repository in browser |
| `status` | Dashboard of notifications, assigned issues, and review requests |
| `codespace` | Codespaces management |
| `project` | GitHub Projects (v2) |
| `org` | Organization operations |
| `cache` | GitHub Actions cache management |
| `ruleset` | Repository rulesets |
| `attestation` | Artifact attestations |
| `completion` | Shell completion scripts |

## Output Formatting

ghc supports the same output formatting flags as `gh`:

```bash
# JSON output with field selection
ghc issue list --json number,title,state

# jq filtering (powered by jaq)
ghc pr list --json title --jq '.[].title'

# Go template formatting
ghc repo list --json name,url --template '{{range .}}{{.name}}: {{.url}}{{"\n"}}{{end}}'
```

## Feature Parity

Tested against `gh` v2.86.0 with 64 real-world scenarios:

| Result | Count | Details |
|--------|-------|---------|
| PASS   | 59    | Identical output to gh |
| DIFF   | 5     | Minor cosmetic differences |
| FAIL   | 0     | No failures |

**Pass rate: 92% exact match, 0% failure**

See [docs/cli-parity-test-report.md](docs/cli-parity-test-report.md) for the full test matrix.

## Architecture

ghc is structured as a Rust workspace with five crates:

```
ghc/
  crates/
    ghc/          # Binary entry point
    ghc-cmd/      # All CLI command implementations (33 groups, 370+ tests)
    ghc-core/     # Shared types: config, auth, IOStreams, JSON/jq/template (350+ tests)
    ghc-api/      # HTTP client for GitHub REST & GraphQL APIs
    ghc-git/      # Git operations (clone, push, credential helpers) (100+ tests)
```

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing with derive macros |
| `reqwest` + `rustls` | HTTP client with pure-Rust TLS |
| `jaq-*` | Full jq expression support |
| `secrecy` | Secure token handling |
| `tokio` | Async runtime |
| `serde` / `serde_json` | Serialization |

## Development

```bash
# Build
cargo build

# Run tests (900+ tests)
cargo test --all

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo +nightly fmt --all
```

## Configuration

ghc uses the same configuration files as `gh`:

- `~/.config/gh/config.yml` - General settings
- `~/.config/gh/hosts.yml` - Authentication tokens

All 11 configuration keys are supported:

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

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2025 Tyr Chen
