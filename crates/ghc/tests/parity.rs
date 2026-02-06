//! Feature parity tests between `gh` (Go CLI) and `ghc` (Rust CLI).
//!
//! These tests compare the output of `gh` and `ghc` for equivalent commands
//! to ensure behavioral parity. They are marked `#[ignore]` by default
//! because they require both `gh` and `ghc` binaries to be available, and
//! may require authentication.
//!
//! Run with: `cargo test -p ghc --test parity -- --ignored`
//! Or use:   `make parity-test`

use std::process::Command;

/// Result of running a CLI command.
#[derive(Debug)]
struct CliOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

/// Run a command and capture its output.
fn run_command(program: &str, args: &[&str]) -> CliOutput {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run `{program}`: {e}"));

    CliOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    }
}

/// Assert that `gh` and `ghc` produce equivalent output for the given args.
///
/// Compares exit code and stdout. Stderr is logged but not compared since
/// progress messages and debug output may differ.
fn assert_parity(args: &[&str]) {
    let gh = run_command("gh", args);
    let ghc = run_command("ghc", args);

    assert_eq!(
        gh.exit_code, ghc.exit_code,
        "exit code mismatch for args {:?}\ngh  stderr: {}\nghc stderr: {}",
        args, gh.stderr, ghc.stderr,
    );

    assert_eq!(
        gh.stdout.trim(),
        ghc.stdout.trim(),
        "stdout mismatch for args {:?}\ngh  stdout: {}\nghc stdout: {}",
        args,
        gh.stdout,
        ghc.stdout,
    );
}

/// Assert that `gh` and `ghc` produce the same exit code (ignoring output
/// differences). Useful for commands where exact output format may differ
/// but the success/failure semantics must match.
fn assert_exit_code_parity(args: &[&str]) {
    let gh = run_command("gh", args);
    let ghc = run_command("ghc", args);

    assert_eq!(
        gh.exit_code, ghc.exit_code,
        "exit code mismatch for args {:?}\ngh  stderr: {}\nghc stderr: {}",
        args, gh.stderr, ghc.stderr,
    );
}

// --- Version & Help ---

#[test]
#[ignore = "requires both gh and ghc binaries"]
fn test_parity_version_flag() {
    // Both should exit 0 with --version
    assert_exit_code_parity(&["--version"]);
}

#[test]
#[ignore = "requires both gh and ghc binaries"]
fn test_parity_help_flag() {
    assert_exit_code_parity(&["--help"]);
}

#[test]
#[ignore = "requires both gh and ghc binaries"]
fn test_parity_unknown_command_exit_code() {
    assert_exit_code_parity(&["nonexistent-command-xyz"]);
}

// --- Repo commands ---

#[test]
#[ignore = "requires both gh and ghc binaries and network access"]
fn test_parity_repo_view_json() {
    assert_parity(&[
        "repo",
        "view",
        "cli/cli",
        "--json",
        "name,owner,description",
    ]);
}

// --- Issue commands ---

#[test]
#[ignore = "requires both gh and ghc binaries and network access"]
fn test_parity_issue_list_json() {
    assert_parity(&[
        "issue",
        "list",
        "-R",
        "cli/cli",
        "--json",
        "number,title,state",
        "-L",
        "3",
    ]);
}

// --- PR commands ---

#[test]
#[ignore = "requires both gh and ghc binaries and network access"]
fn test_parity_pr_list_json() {
    assert_parity(&[
        "pr",
        "list",
        "-R",
        "cli/cli",
        "--json",
        "number,title,state",
        "-L",
        "3",
    ]);
}

// --- Auth commands ---

#[test]
#[ignore = "requires both gh and ghc binaries and authentication"]
fn test_parity_auth_status_exit_code() {
    // auth status should exit the same way
    assert_exit_code_parity(&["auth", "status"]);
}
