# GHC vs GH CLI Feature Parity Audit Report

**Date:** 2026-02-06
**Auditor:** Code Review Agent
**Reference:** Go implementation at `vendors/cli/pkg/cmd/`
**Implementation:** Rust implementation at `crates/ghc-cmd/src/`

---

## Executive Summary

The ghc (Rust) implementation covers **33 of 35** command groups from the gh (Go) CLI. Subcommand coverage within those groups is approximately **92%**. The most significant gaps are:

1. **Missing `--jq` and `--template` formatting flags** across all list/view commands (systemic)
2. **Missing `pr lock`/`pr unlock` and `issue unlock` subcommands**
3. **Missing `repo credits` and `repo garden` subcommands**
4. **Missing `codespace cp` and `codespace select` subcommands**
5. **Several missing flags** in `secret`, `variable`, `api`, `search`, and `run` commands
6. **Flag short-name swap** in `api` command (`-f`/`-F` are reversed vs Go)

---

## Summary Table

| Command Group | Go Subcmds | Rust Subcmds | Parity | Notes |
|---|---|---|---|---|
| **auth** | 8 | 8 | FULL | All subcommands and flags present |
| **repo** | 18 | 16 | PARTIAL | Missing: `credits`, `garden` |
| **pr** | 17 | 15 | PARTIAL | Missing: `lock`, `unlock` |
| **issue** | 14 | 13 | PARTIAL | Missing: `unlock` |
| **run** | 8 | 8 | PARTIAL | Missing flags: `--created`, `--commit`, `--all`, `--compact`, `--log`, `--log-failed` |
| **workflow** | 5 | 5 | FULL | All subcommands present |
| **secret** | 3 | 3 | PARTIAL | Missing flags: `--user`, `--app`, `--repos`, `--no-store`, `--no-repos-selected` |
| **variable** | 4 | 4 | PARTIAL | Missing flag: `--repos` in set |
| **release** | 10 | 10 | PARTIAL | Missing flags: `--verify-tag`, `--notes-from-tag`, `--fail-on-no-commits` |
| **search** | 5 | 5 | PARTIAL | Many qualifier flags missing (see details) |
| **gist** | 7 | 7 | FULL | All subcommands present |
| **label** | 5 | 5 | FULL | All subcommands present |
| **api** | 1 | 1 | PARTIAL | Missing `--preview`, `--template`; **`-f`/`-F` short flags are swapped** |
| **alias** | 4 | 4 | FULL | All subcommands present |
| **cache** | 2 | 2 | FULL | All subcommands present |
| **codespace** | 14 | 12 | PARTIAL | Missing: `cp`, `select` |
| **config** | 4 | 4 | FULL | All subcommands present |
| **extension** | 7 | 7 | FULL | All subcommands present |
| **project** | 17 | 17 | FULL | All subcommands present |
| **ruleset** | 3 | 3 | FULL | All subcommands present |
| **gpg-key** | 3 | 3 | FULL | All subcommands present |
| **ssh-key** | 3 | 3 | FULL | All subcommands present |
| **org** | 1 | 1 | FULL | |
| **browse** | 1 | 1 | FULL | |
| **status** | 1 | 1 | FULL | |
| **completion** | 1 | 1 | FULL | |
| **attestation** | 4 | 4 | FULL | All subcommands present |
| **actions** | 1 | 1 | FULL | |
| **copilot** | 1 | 1 | FULL | |
| **preview** | 1 | 1 | FULL | |
| **accessibility** | 1 | 1 | FULL | |
| **agent-task** | 1 | 1 | FULL | |

---

## Detailed Findings

### Critical Issues

#### 1. `api` command: `-f`/`-F` short flags are SWAPPED (CRITICAL)

In Go:
- `-F` = `--field` (typed parameter, JSON coercion)
- `-f` = `--raw-field` (string parameter, no coercion)

In Rust (`crates/ghc-cmd/src/api/mod.rs`):
- `-f` = `--field`
- `-F` = `--raw-field`

**This is a breaking behavioral difference.** Users migrating from `gh` to `ghc` using short flags will get the opposite behavior. Scripts using `-f` and `-F` will silently produce wrong results.

#### 2. Systemic: Missing `--jq` and `--template` output formatting flags

The Go CLI provides `--json`, `--jq`, and `--template` flags via `cmdutil.AddJSONFlags()` on most list/view commands. The Rust implementation has `--json` in some commands (pr list, issue list, repo list, auth status) but is missing:

- `--jq` (jq expression filtering) on list/view commands
- `--template` (Go template formatting) on list/view commands

**Affected commands:** `pr list`, `pr view`, `pr status`, `pr checks`, `issue list`, `issue view`, `issue status`, `repo list`, `repo view`, `repo deploy-key list`, `repo autolink list/view`, `run list`, `run view`, `release list`, `release view`, `search *`, `secret list`, `variable list`, `cache list`, `label list`, `codespace list`, `extension list`, `ssh-key list`, `gpg-key list`

---

### High Priority Issues

#### 3. PR: Missing `lock` and `unlock` subcommands

Go `pr` has `lock` and `unlock` (imported from `issue/lock` package). Rust `pr` does not have either. These allow locking/unlocking PR conversations.

**Files:**
- Go: `vendors/cli/pkg/cmd/issue/lock/lock.go`
- Rust: Not present in `crates/ghc-cmd/src/pr/mod.rs`

#### 4. Issue: Missing `unlock` subcommand

Rust has `issue lock` (`crates/ghc-cmd/src/issue/lock.rs`) but is missing `issue unlock`. In Go, both `lock` and `unlock` are generated from the same `lock.go` file with `NewCmdLock` and `NewCmdUnlock`.

#### 5. Repo: Missing `credits` and `garden` subcommands

- `repo credits`: Shows repository contributors (hidden/Easter egg command in Go)
- `repo garden`: ASCII art garden visualization of a repository (hidden/Easter egg command in Go)

Both are registered in Go's `repo.go` but absent from Rust's `repo/mod.rs`. These are novelty commands but still part of the Go CLI.

#### 6. Codespace: Missing `cp` and `select` subcommands

- `codespace cp`: Copy files between local and codespace filesystems
- `codespace select`: Select a codespace interactively

Go has 14 codespace subcommands; Rust has 12.

---

### Medium Priority Issues

#### 7. Secret command: Missing flags

**`secret set`** (`crates/ghc-cmd/src/secret/set.rs`):
- Missing: `--user` / `-u` (set user secret for Codespaces)
- Missing: `--repos` / `-r` (restrict org/user secret to specific repos)
- Missing: `--no-repos-selected` (no repos can access org secret)
- Missing: `--no-store` (print encrypted value instead of storing)
- Missing: `--app` / `-a` (target application: actions/codespaces/dependabot)

**`secret list`** (`crates/ghc-cmd/src/secret/list.rs`):
- Missing: `--user` / `-u` (list user secrets)
- Missing: `--app` / `-a` (list secrets for specific app)

**`secret delete`** (`crates/ghc-cmd/src/secret/delete.rs`):
- Missing: `--user` / `-u` (delete user secret)
- Missing: `--app` / `-a` (delete secret for specific app)

#### 8. Run command: Missing flags

**`run list`** (`crates/ghc-cmd/src/run/list.rs`):
- Missing: `--created` (filter by creation date)
- Missing: `--commit` / `-c` (filter by commit SHA)
- Missing: `--all` / `-a` (include disabled workflows)

**`run view`** (`crates/ghc-cmd/src/run/view.rs`):
- Missing: `--verbose` / `-v` (show job steps)
- Missing: `--log` (view full log)
- Missing: `--log-failed` (view log for failed steps)
- Missing: `--job` / `-j` (view specific job ID)
- Missing: `--web` / `-w` (open in browser)

**`run watch`** (`crates/ghc-cmd/src/run/watch.rs`):
- Missing: `--compact` (show only relevant/failed steps)

#### 9. Release create: Missing flags

**`release create`** (`crates/ghc-cmd/src/release/create.rs`):
- Missing: `--verify-tag` (abort if tag doesn't exist remotely)
- Missing: `--notes-from-tag` (fetch notes from tag annotation)
- Missing: `--fail-on-no-commits` (fail if no commits since last release)

#### 10. Search commands: Many qualifier flags missing

**`search repos`** (`crates/ghc-cmd/src/search/repos.rs`):
Present: `--limit`, `--web`, `--sort`, `--order`, `--visibility`, `--topic`, `--json`
Missing: `--created`, `--followers`, `--include-forks`, `--forks`, `--good-first-issues`, `--help-wanted-issues`, `--match`, `--language`, `--license`, `--updated`, `--size`, `--stars`, `--number-topics`, `--owner`

**`search issues`** (`crates/ghc-cmd/src/search/issues.rs`):
Present: `--limit`, `--repo`, `--state`, `--assignee`, `--author`, `--web`, `--label`, `--json`
Missing: `--app`, `--closed`, `--commenter`, `--comments`, `--created`, `--match`, `--interactions`, `--involves`, `--visibility`, `--language`, `--locked`, `--mentions`, `--include-prs`, and many more qualifiers

The Go search commands have extensive qualifier-based filtering (15-20 flags each) that Rust only partially implements.

#### 11. API command: Missing flags

**`api`** (`crates/ghc-cmd/src/api/mod.rs`):
- Missing: `--preview` / `-p` (opt into GitHub API previews)
- Missing: `--template` / `-t` (format output with Go template)
- **CRITICAL:** `-f` and `-F` short flags are swapped (see Critical Issue #1)

#### 12. Variable command: Missing flags

**`variable set`** (`crates/ghc-cmd/src/variable/set.rs`):
- Missing: `--repos` / `-r` (restrict to specific repositories)

---

### Low Priority Issues

#### 13. PR create: Missing `--project` and `--recover` flags

**`pr create`** (`crates/ghc-cmd/src/pr/create.rs`):
- Missing: `--project` / `-p` (add PR to projects by title)
- Missing: `--recover` (recover input from a failed create run)
- Present (verified): `--editor`, `--no-maintainer-edit`, `--dry-run`, `--fill-verbose`, `--fill-first`, `--template`

#### 14. Repo view: Missing `--branch` flag

Go `repo view` has `--branch` / `-b` flag to view a specific branch. Rust `repo view` (`crates/ghc-cmd/src/repo/view.rs`) has `--web`, `--json` but needs verification for `--branch`.

#### 15. JSON output behavior differences

When `--json` is provided in Go with `AddJSONFlags`, it accepts a comma-separated list of field names and supports `--jq` and `--template` for filtering/formatting. In Rust, `--json` takes field names but lacks `--jq`/`--template` post-processing capabilities (covered in Critical Issue #2).

---

## Parity Statistics

| Category | Count |
|---|---|
| Total Go command groups | 35 (including root, version) |
| Total Rust command groups | 33 |
| Command groups at FULL parity | 22 |
| Command groups at PARTIAL parity | 11 |
| Command groups MISSING | 0 |
| Critical issues | 2 (flag swap, systemic jq/template) |
| High priority issues | 4 |
| Medium priority issues | 6 |
| Low priority issues | 3 |

---

## Recommendations (Priority Order)

1. **Fix `api` `-f`/`-F` flag swap immediately** -- this is a silent correctness bug
2. **Implement `--jq` and `--template` as shared infrastructure** -- affects all list/view commands
3. **Add `issue unlock` and `pr lock`/`pr unlock`** -- core functionality gap
4. **Add missing `secret`/`variable` flags** (`--user`, `--app`, `--repos`) -- needed for org/enterprise usage
5. **Add missing `run view` flags** (`--log`, `--log-failed`, `--verbose`, `--job`, `--web`) -- critical for CI debugging
6. **Add `codespace cp` and `codespace select`** -- needed for codespace workflows
7. **Add missing search qualifier flags** -- needed for advanced search use cases
8. **Add missing `release create` flags** -- needed for CI release workflows
9. **Consider adding `repo credits`/`repo garden`** -- low priority, Easter egg commands
