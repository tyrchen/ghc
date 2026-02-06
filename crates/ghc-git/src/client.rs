//! Git client that wraps the git command-line tool.
//!
//! Maps from Go's `git/client.go`.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use regex::Regex;
use tokio::process::Command;
use tracing::instrument;

use crate::errors::GitError;
use crate::remote::Remote;

/// Configuration key for tracking the PR target branch.
pub const MERGE_BASE_CONFIG: &str = "gh-merge-base";

/// A git reference (commit hash + name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Commit hash.
    pub hash: String,
    /// Full ref name (e.g., `refs/heads/main`).
    pub name: String,
}

/// A git commit with metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full SHA hash.
    pub sha: String,
    /// Commit title (first line of message).
    pub title: String,
    /// Commit body (rest of the message).
    pub body: String,
}

/// Branch tracking configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchConfig {
    /// Remote name (if a named remote).
    pub remote_name: String,
    /// Remote URL (if a URL-based remote).
    pub remote_url: String,
    /// Merge ref (e.g., `refs/heads/main`).
    pub merge_ref: String,
    /// Push remote name.
    pub push_remote_name: String,
    /// Push remote URL.
    pub push_remote_url: String,
    /// Custom merge base branch for PRs.
    pub merge_base: String,
}

/// The default push behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushDefault {
    /// Don't push anything.
    Nothing,
    /// Push the current branch to its upstream.
    Current,
    /// Push the current branch to its upstream tracking branch.
    Upstream,
    /// Alias for upstream.
    Tracking,
    /// Push the current branch to the remote branch with the same name.
    Simple,
    /// Push all branches with matching names.
    Matching,
}

impl PushDefault {
    /// Parse a push.default value from a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not a valid push.default setting.
    pub fn parse(s: &str) -> Result<Self, GitError> {
        match s {
            "nothing" => Ok(Self::Nothing),
            "current" => Ok(Self::Current),
            "upstream" => Ok(Self::Upstream),
            "tracking" => Ok(Self::Tracking),
            "simple" => Ok(Self::Simple),
            "matching" => Ok(Self::Matching),
            _ => Err(GitError::CommandFailed {
                command: "config".to_string(),
                message: format!("unknown push.default value: {s}"),
                exit_code: None,
            }),
        }
    }
}

/// Structured remote tracking ref (e.g., `refs/remotes/origin/main`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTrackingRef {
    /// Remote name.
    pub remote: String,
    /// Branch name.
    pub branch: String,
}

impl RemoteTrackingRef {
    /// Parse a string like `refs/remotes/<remote>/<branch>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the string does not match the expected format.
    pub fn parse(s: &str) -> Result<Self, GitError> {
        let prefix = "refs/remotes/";
        let rest = s.strip_prefix(prefix).ok_or_else(|| GitError::CommandFailed {
            command: "rev-parse".to_string(),
            message: format!("remote tracking branch must have format refs/remotes/<remote>/<branch> but was: {s}"),
            exit_code: None,
        })?;

        let (remote, branch) = rest.split_once('/').ok_or_else(|| GitError::CommandFailed {
            command: "rev-parse".to_string(),
            message: format!("remote tracking branch must have format refs/remotes/<remote>/<branch> but was: {s}"),
            exit_code: None,
        })?;

        Ok(Self {
            remote: remote.to_string(),
            branch: branch.to_string(),
        })
    }
}

impl std::fmt::Display for RemoteTrackingRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "refs/remotes/{}/{}", self.remote, self.branch)
    }
}

/// Credential pattern for authenticated git commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialPattern {
    /// Match all hosts (less secure, for backward compatibility).
    AllMatching,
    /// Match a specific host pattern (e.g., `https://github.com`).
    Host(String),
}

impl CredentialPattern {
    /// Create a credential pattern from a git remote URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be parsed.
    pub fn from_git_url(git_url: &str) -> Result<Self, GitError> {
        let normalized =
            crate::url_parser::parse_url(git_url).map_err(|e| GitError::CommandFailed {
                command: "credential".to_string(),
                message: format!("failed to parse remote URL: {e}"),
                exit_code: None,
            })?;
        let host = normalized.host_str().unwrap_or("github.com");
        Ok(Self::from_host(host))
    }

    /// Create a credential pattern from a hostname.
    pub fn from_host(host: &str) -> Self {
        let prefix = ghc_core::instance::host_prefix(host);
        Self::Host(prefix.trim_end_matches('/').to_string())
    }
}

/// Client for executing git commands.
#[derive(Debug, Clone)]
pub struct GitClient {
    /// Path to the git binary.
    git_path: PathBuf,
    /// Working directory for git commands.
    repo_dir: Option<PathBuf>,
    /// Path to the ghc binary (for credential helper).
    ghc_path: Option<PathBuf>,
}

impl GitClient {
    /// Create a new git client using the system git.
    ///
    /// # Errors
    ///
    /// Returns an error if git is not found in PATH.
    pub fn new() -> Result<Self, GitError> {
        let git_path = which::which("git").map_err(|_| GitError::NotFound)?;

        Ok(Self {
            git_path,
            repo_dir: None,
            ghc_path: None,
        })
    }

    /// Set the working directory.
    #[must_use]
    pub fn with_repo_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.repo_dir = Some(dir.into());
        self
    }

    /// Set the ghc binary path for credential helper.
    #[must_use]
    pub fn with_ghc_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.ghc_path = Some(path.into());
        self
    }

    /// Get the repository directory, if set.
    pub fn repo_dir(&self) -> Option<&Path> {
        self.repo_dir.as_deref()
    }

    /// Execute a git command and return stdout.
    #[instrument(skip(self), fields(args = ?args))]
    async fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let mut cmd = Command::new(&self.git_path);
        cmd.args(args);

        if let Some(ref dir) = self.repo_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let command = args.first().copied().unwrap_or("").to_string();
            return Err(GitError::CommandFailed {
                command,
                message: stderr.trim().to_string(),
                exit_code: output.status.code(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Execute a git command with extra pre-args (inserted before the main args).
    #[instrument(skip(self), fields(pre_args = ?pre_args, args = ?args))]
    async fn run_with_pre_args(
        &self,
        pre_args: &[&str],
        args: &[&str],
    ) -> Result<String, GitError> {
        let mut full_args: Vec<&str> = pre_args.to_vec();
        full_args.extend_from_slice(args);
        self.run(&full_args).await
    }

    /// Execute an authenticated git command using ghc as credential helper.
    #[instrument(skip(self), fields(pattern = ?pattern, args = ?args))]
    async fn run_authenticated(
        &self,
        pattern: &CredentialPattern,
        args: &[&str],
    ) -> Result<String, GitError> {
        let ghc_path = self
            .ghc_path
            .as_deref()
            .map_or_else(|| "ghc".to_string(), |p| p.to_string_lossy().to_string());

        let cred_helper = format!("!{ghc_path} auth git-credential");

        let pre_args: Vec<String> = match pattern {
            CredentialPattern::AllMatching => {
                vec![
                    "-c".to_string(),
                    "credential.helper=".to_string(),
                    "-c".to_string(),
                    format!("credential.helper={cred_helper}"),
                ]
            }
            CredentialPattern::Host(p) => {
                vec![
                    "-c".to_string(),
                    format!("credential.{p}.helper="),
                    "-c".to_string(),
                    format!("credential.{p}.helper={cred_helper}"),
                ]
            }
        };

        let pre_refs: Vec<&str> = pre_args.iter().map(String::as_str).collect();
        self.run_with_pre_args(&pre_refs, args).await
    }

    /// Execute an authenticated git command that may write to stderr (e.g., fetch, push, pull).
    /// Returns Ok on success even if there is no stdout.
    #[instrument(skip(self), fields(pattern = ?pattern, args = ?args))]
    async fn run_authenticated_passthrough(
        &self,
        pattern: &CredentialPattern,
        args: &[&str],
    ) -> Result<(), GitError> {
        let ghc_path = self
            .ghc_path
            .as_deref()
            .map_or_else(|| "ghc".to_string(), |p| p.to_string_lossy().to_string());

        let cred_helper = format!("!{ghc_path} auth git-credential");

        let mut full_args: Vec<String> = match pattern {
            CredentialPattern::AllMatching => {
                vec![
                    "-c".to_string(),
                    "credential.helper=".to_string(),
                    "-c".to_string(),
                    format!("credential.helper={cred_helper}"),
                ]
            }
            CredentialPattern::Host(p) => {
                vec![
                    "-c".to_string(),
                    format!("credential.{p}.helper="),
                    "-c".to_string(),
                    format!("credential.{p}.helper={cred_helper}"),
                ]
            }
        };

        for arg in args {
            full_args.push((*arg).to_string());
        }

        let mut cmd = Command::new(&self.git_path);
        let str_args: Vec<&str> = full_args.iter().map(String::as_str).collect();
        cmd.args(&str_args);

        if let Some(ref dir) = self.repo_dir {
            cmd.current_dir(dir);
        }

        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        let status = cmd.status().await?;

        if !status.success() {
            let command = args.first().copied().unwrap_or("").to_string();
            return Err(GitError::CommandFailed {
                command,
                message: String::new(),
                exit_code: status.code(),
            });
        }

        Ok(())
    }

    // =====================================================================
    // Local operations (no authentication needed)
    // =====================================================================

    /// Get the current branch name.
    ///
    /// # Errors
    ///
    /// Returns `NotOnAnyBranch` if HEAD is detached.
    pub async fn current_branch(&self) -> Result<String, GitError> {
        match self.run(&["symbolic-ref", "--quiet", "HEAD"]).await {
            Ok(output) => {
                let branch = first_line(&output);
                Ok(branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_string())
            }
            Err(GitError::CommandFailed { message, .. }) if message.is_empty() => {
                Err(GitError::NotOnAnyBranch)
            }
            Err(e) => Err(e),
        }
    }

    /// List all remotes with their URLs and resolved status.
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub async fn remotes(&self) -> Result<Vec<Remote>, GitError> {
        let output = self.run(&["remote", "-v"]).await?;
        let mut remotes = Remote::parse_remotes(&output);

        // Populate gh-resolved config
        match self
            .run(&["config", "--get-regexp", r"^remote\..*\.gh-resolved$"])
            .await
        {
            Ok(config_output) => {
                Remote::populate_resolved(&mut remotes, &config_output);
            }
            Err(GitError::CommandFailed {
                exit_code: Some(1), ..
            }) => {
                // No resolved remotes found, that's fine
            }
            Err(e) => return Err(e),
        }

        Ok(remotes)
    }

    /// Resolve fully-qualified refs to commit hashes.
    ///
    /// # Errors
    ///
    /// Returns an error if the git command fails.
    pub async fn show_refs(&self, refs: &[&str]) -> Result<Vec<Ref>, GitError> {
        let mut args = vec!["show-ref", "--verify", "--"];
        args.extend_from_slice(refs);

        // show-ref may return exit code 1 for missing refs but still produce output
        let output = match self.run(&args).await {
            Ok(out) => out,
            Err(GitError::CommandFailed { message, .. }) => message,
            Err(e) => return Err(e),
        };

        let mut verified = Vec::new();
        for line in output.lines() {
            if let Some((hash, name)) = line.split_once(' ') {
                verified.push(Ref {
                    hash: hash.to_string(),
                    name: name.to_string(),
                });
            }
        }

        Ok(verified)
    }

    /// Get a git config value.
    ///
    /// # Errors
    ///
    /// Returns an error if the config key is not found.
    pub async fn config_get(&self, key: &str) -> Result<String, GitError> {
        match self.run(&["config", key]).await {
            Ok(output) => Ok(first_line(&output).to_string()),
            Err(GitError::CommandFailed {
                exit_code: Some(1), ..
            }) => Err(GitError::CommandFailed {
                command: "config".to_string(),
                message: format!("unknown config key {key}"),
                exit_code: Some(1),
            }),
            Err(e) => Err(e),
        }
    }

    /// Set a git config value.
    ///
    /// # Errors
    ///
    /// Returns an error if setting the config fails.
    pub async fn config_set(&self, key: &str, value: &str) -> Result<(), GitError> {
        self.run(&["config", key, value]).await?;
        Ok(())
    }

    /// Count uncommitted changes in the working directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the status check fails.
    pub async fn uncommitted_change_count(&self) -> Result<usize, GitError> {
        let output = self.run(&["status", "--porcelain"]).await?;
        Ok(output.lines().filter(|l| !l.is_empty()).count())
    }

    /// Get commits between two refs.
    ///
    /// # Errors
    ///
    /// Returns `NoCommits` if no commits are found between the refs.
    pub async fn commits(&self, base_ref: &str, head_ref: &str) -> Result<Vec<Commit>, GitError> {
        let range = format!("{base_ref}...{head_ref}");
        let output = self
            .run(&[
                "-c",
                "log.ShowSignature=false",
                "log",
                "--pretty=format:%H%x00%s%x00%b%x00",
                "--cherry",
                &range,
            ])
            .await?;

        let re = Regex::new(r"(?m)^[0-9a-fA-F]{7,40}\x00.*?\x00[\S\s]*?\x00$").map_err(|e| {
            GitError::CommandFailed {
                command: "log".to_string(),
                message: format!("invalid regex: {e}"),
                exit_code: None,
            }
        })?;

        let mut commits = Vec::new();
        for m in re.find_iter(&output) {
            let parts: Vec<&str> = m.as_str().split('\x00').collect();
            if parts.len() >= 3 {
                commits.push(Commit {
                    sha: parts[0].to_string(),
                    title: parts[1].to_string(),
                    body: parts[2].to_string(),
                });
            }
        }

        if commits.is_empty() {
            return Err(GitError::NoCommits {
                base_ref: base_ref.to_string(),
                head_ref: head_ref.to_string(),
            });
        }

        Ok(commits)
    }

    /// Get the last commit on HEAD.
    ///
    /// # Errors
    ///
    /// Returns an error if the lookup fails.
    pub async fn last_commit(&self) -> Result<Commit, GitError> {
        let output = self.lookup_commit("HEAD", "%H,%s").await?;
        let (sha, title) = output.split_once(',').unwrap_or((&output, ""));
        Ok(Commit {
            sha: sha.to_string(),
            title: title.trim().to_string(),
            body: String::new(),
        })
    }

    /// Get the body of a specific commit.
    ///
    /// # Errors
    ///
    /// Returns an error if the lookup fails.
    pub async fn commit_body(&self, sha: &str) -> Result<String, GitError> {
        self.lookup_commit(sha, "%b").await
    }

    async fn lookup_commit(&self, sha: &str, format: &str) -> Result<String, GitError> {
        let format_arg = format!("--pretty=format:{format}");
        self.run(&[
            "-c",
            "log.ShowSignature=false",
            "show",
            "-s",
            &format_arg,
            sha,
        ])
        .await
    }

    /// Read branch config (remote, merge, pushremote, gh-merge-base).
    ///
    /// # Errors
    ///
    /// Returns an empty `BranchConfig` if no config exists for the branch.
    pub async fn read_branch_config(&self, branch: &str) -> Result<BranchConfig, GitError> {
        let prefix = regex::escape(&format!("branch.{branch}."));
        let pattern = format!("^{prefix}(remote|merge|pushremote|{MERGE_BASE_CONFIG})$");

        match self.run(&["config", "--get-regexp", &pattern]).await {
            Ok(output) => Ok(parse_branch_config(&output)),
            Err(GitError::CommandFailed {
                exit_code: Some(1), ..
            }) => Ok(BranchConfig::default()),
            Err(e) => Err(e),
        }
    }

    /// Set a named config value on a branch.
    ///
    /// # Errors
    ///
    /// Returns an error if the config cannot be set.
    pub async fn set_branch_config(
        &self,
        branch: &str,
        name: &str,
        value: &str,
    ) -> Result<(), GitError> {
        let key = format!("branch.{branch}.{name}");
        self.run(&["config", &key, value]).await?;
        Ok(())
    }

    /// Get the push.default setting.
    ///
    /// # Errors
    ///
    /// Returns `PushDefault::Simple` if not configured (git default since 2.0).
    pub async fn push_default(&self) -> Result<PushDefault, GitError> {
        match self.config_get("push.default").await {
            Ok(val) => PushDefault::parse(&val),
            Err(e) if e.is_exit_code_1() => Ok(PushDefault::Simple),
            Err(e) => Err(e),
        }
    }

    /// Get the remote.pushDefault setting.
    pub async fn remote_push_default(&self) -> Result<String, GitError> {
        match self.config_get("remote.pushDefault").await {
            Ok(val) => Ok(val),
            Err(e) if e.is_exit_code_1() => Ok(String::new()),
            Err(e) => Err(e),
        }
    }

    /// Get the `@{push}` revision for a branch.
    pub async fn push_revision(&self, branch: &str) -> Result<RemoteTrackingRef, GitError> {
        let rev = format!("{branch}@{{push}}");
        let output = self.rev_parse(&["--symbolic-full-name", &rev]).await?;
        RemoteTrackingRef::parse(first_line(&output))
    }

    /// Get the tracking branch for a local branch.
    ///
    /// # Errors
    ///
    /// Returns an error if no tracking branch is set.
    pub async fn tracking_branch(&self, branch: &str) -> Result<String, GitError> {
        self.run(&["config", &format!("branch.{branch}.merge")])
            .await
    }

    /// Check if a local branch exists.
    pub async fn has_local_branch(&self, branch: &str) -> bool {
        let ref_name = format!("refs/heads/{branch}");
        self.rev_parse(&["--verify", &ref_name]).await.is_ok()
    }

    /// Get tracking branch names, optionally filtered by prefix.
    pub async fn tracking_branch_names(&self, prefix: &str) -> Vec<String> {
        let mut args = vec!["branch", "-r", "--format", "%(refname:strip=3)"];

        let list_pattern;
        if !prefix.is_empty() {
            let escaped = escape_glob(prefix);
            list_pattern = format!("*/{escaped}*");
            args.push("--list");
            args.push(&list_pattern);
        }

        match self.run(&args).await {
            Ok(output) => output.lines().map(String::from).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Get the top-level directory of the repository.
    ///
    /// # Errors
    ///
    /// Returns an error if not in a git repo.
    pub async fn top_level_dir(&self) -> Result<PathBuf, GitError> {
        let output = self.rev_parse(&["--show-toplevel"]).await?;
        Ok(PathBuf::from(first_line(&output)))
    }

    /// Get the .git directory path.
    ///
    /// # Errors
    ///
    /// Returns an error if not in a git repo.
    pub async fn git_dir(&self) -> Result<PathBuf, GitError> {
        let output = self.rev_parse(&["--git-dir"]).await?;
        Ok(PathBuf::from(first_line(&output)))
    }

    /// Get the path from the repo root to the current directory.
    pub async fn path_from_root(&self) -> String {
        match self.rev_parse(&["--show-prefix"]).await {
            Ok(output) => {
                let path = first_line(&output);
                if path.is_empty() {
                    String::new()
                } else {
                    path.trim_end_matches('/').to_string()
                }
            }
            Err(_) => String::new(),
        }
    }

    /// Check if the working directory is a git repository.
    ///
    /// # Errors
    ///
    /// Returns an error only if the check itself fails (not for non-repo directories).
    pub async fn is_repo(&self) -> Result<bool, GitError> {
        match self.git_dir().await {
            Ok(_) => Ok(true),
            Err(GitError::CommandFailed { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Checkout a branch or ref.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkout fails.
    pub async fn checkout(&self, ref_name: &str) -> Result<(), GitError> {
        self.run(&["checkout", ref_name]).await?;
        Ok(())
    }

    /// Create and checkout a new branch tracking a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if branch creation fails.
    pub async fn checkout_new_branch(
        &self,
        remote_name: &str,
        branch: &str,
    ) -> Result<(), GitError> {
        let track = format!("{remote_name}/{branch}");
        self.run(&["checkout", "-b", branch, "--track", &track])
            .await?;
        Ok(())
    }

    /// Delete a local branch.
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be deleted.
    pub async fn delete_local_branch(&self, branch: &str) -> Result<(), GitError> {
        self.run(&["branch", "-D", branch]).await?;
        Ok(())
    }

    /// Delete a local tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the tag cannot be deleted.
    pub async fn delete_local_tag(&self, tag: &str) -> Result<(), GitError> {
        self.run(&["tag", "-d", tag]).await?;
        Ok(())
    }

    /// Stage files for commit.
    ///
    /// # Errors
    ///
    /// Returns an error if git add fails.
    pub async fn add(&self, paths: &[&str]) -> Result<(), GitError> {
        let mut args = vec!["add"];
        args.extend_from_slice(paths);
        self.run(&args).await?;
        Ok(())
    }

    /// Create a commit with the given message.
    ///
    /// # Errors
    ///
    /// Returns an error if the commit fails.
    pub async fn commit(&self, message: &str) -> Result<(), GitError> {
        self.run(&["commit", "-m", message]).await?;
        Ok(())
    }

    /// Show a diff between two refs.
    ///
    /// # Errors
    ///
    /// Returns an error if the diff fails.
    pub async fn diff(&self, base: &str, head: &str) -> Result<String, GitError> {
        let range = format!("{base}...{head}");
        self.run(&["diff", &range]).await
    }

    /// Show the git log for a range of commits.
    ///
    /// # Errors
    ///
    /// Returns an error if the log fails.
    pub async fn log(
        &self,
        base: &str,
        head: &str,
        max_count: Option<usize>,
    ) -> Result<String, GitError> {
        let range = format!("{base}...{head}");
        let mut args = vec!["log", "--oneline", &range];
        let max_str;
        if let Some(n) = max_count {
            max_str = format!("--max-count={n}");
            args.push(&max_str);
        }
        self.run(&args).await
    }

    /// Merge a ref into the current branch.
    ///
    /// # Errors
    ///
    /// Returns an error if the merge fails.
    pub async fn merge(&self, ref_name: &str, ff_only: bool) -> Result<(), GitError> {
        let mut args = vec!["merge"];
        if ff_only {
            args.push("--ff-only");
        }
        args.push(ref_name);
        self.run(&args).await?;
        Ok(())
    }

    /// Stash current changes.
    ///
    /// # Errors
    ///
    /// Returns an error if stashing fails.
    pub async fn stash_push(&self, message: Option<&str>) -> Result<(), GitError> {
        let mut args = vec!["stash", "push"];
        if let Some(msg) = message {
            args.push("-m");
            args.push(msg);
        }
        self.run(&args).await?;
        Ok(())
    }

    /// Pop the most recent stash.
    ///
    /// # Errors
    ///
    /// Returns an error if popping fails.
    pub async fn stash_pop(&self) -> Result<(), GitError> {
        self.run(&["stash", "pop"]).await?;
        Ok(())
    }

    /// Set the `gh-resolved` config key for a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if setting the config fails.
    pub async fn set_remote_resolution(
        &self,
        name: &str,
        resolution: &str,
    ) -> Result<(), GitError> {
        let key = format!("remote.{name}.gh-resolved");
        self.run(&["config", "--add", &key, resolution]).await?;
        Ok(())
    }

    /// Unset the `gh-resolved` config key for a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if unsetting the config fails.
    pub async fn unset_remote_resolution(&self, name: &str) -> Result<(), GitError> {
        let key = format!("remote.{name}.gh-resolved");
        self.run(&["config", "--unset", &key]).await?;
        Ok(())
    }

    /// Update a remote's URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the update fails.
    pub async fn update_remote_url(&self, name: &str, url: &str) -> Result<(), GitError> {
        self.run(&["remote", "set-url", name, url]).await?;
        Ok(())
    }

    /// Set the fetch refspec branches for a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn set_remote_branches(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        self.run(&["remote", "set-branches", remote, refspec])
            .await?;
        Ok(())
    }

    /// Add a new remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the remote cannot be added.
    pub async fn add_remote(
        &self,
        name: &str,
        url: &str,
        tracking_branches: &[&str],
    ) -> Result<(), GitError> {
        let mut args = vec!["remote", "add"];
        for branch in tracking_branches {
            args.push("-t");
            args.push(branch);
        }
        args.push(name);
        args.push(url);
        self.run(&args).await?;
        Ok(())
    }

    /// Remove a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the remote cannot be removed.
    pub async fn remove_remote(&self, name: &str) -> Result<(), GitError> {
        self.run(&["remote", "remove", name]).await?;
        Ok(())
    }

    /// Rename a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails.
    pub async fn rename_remote(&self, old_name: &str, new_name: &str) -> Result<(), GitError> {
        self.run(&["remote", "rename", old_name, new_name]).await?;
        Ok(())
    }

    // =====================================================================
    // Network operations (need authentication)
    // =====================================================================

    /// Fetch from a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the fetch fails.
    pub async fn fetch(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        let mut args = vec!["fetch", remote];
        if !refspec.is_empty() {
            args.push(refspec);
        }
        self.run_authenticated_passthrough(&CredentialPattern::AllMatching, &args)
            .await
    }

    /// Pull changes from a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the pull fails.
    pub async fn pull(&self, remote: &str, branch: &str) -> Result<(), GitError> {
        let mut args = vec!["pull", "--ff-only"];
        if !remote.is_empty() && !branch.is_empty() {
            args.push(remote);
            args.push(branch);
        }
        self.run_authenticated_passthrough(&CredentialPattern::AllMatching, &args)
            .await
    }

    /// Push to a remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the push fails.
    pub async fn push(&self, remote: &str, refspec: &str) -> Result<(), GitError> {
        self.run_authenticated_passthrough(
            &CredentialPattern::AllMatching,
            &["push", "--set-upstream", remote, refspec],
        )
        .await
    }

    /// Clone a repository.
    ///
    /// # Errors
    ///
    /// Returns an error if the clone fails. Returns the target directory name.
    pub async fn clone(&self, clone_url: &str, extra_args: &[&str]) -> Result<String, GitError> {
        let pattern = CredentialPattern::from_git_url(clone_url)?;

        let (mut clone_args, target) = parse_clone_args(extra_args);
        clone_args.push(clone_url.to_string());

        let final_target = if target.is_empty() {
            let base = clone_url
                .rsplit('/')
                .next()
                .unwrap_or(clone_url)
                .trim_end_matches(".git");
            let mut t = base.to_string();
            if clone_args.iter().any(|a| a == "--bare") {
                t.push_str(".git");
            }
            t
        } else {
            clone_args.push(target.clone());
            target
        };

        let mut all_args = vec!["clone".to_string()];
        all_args.extend(clone_args);

        let refs: Vec<&str> = all_args.iter().map(String::as_str).collect();
        self.run_authenticated(&pattern, &refs).await?;

        Ok(final_target)
    }

    /// Run `rev-parse` with the given arguments.
    async fn rev_parse(&self, args: &[&str]) -> Result<String, GitError> {
        let mut full_args = vec!["rev-parse"];
        full_args.extend_from_slice(args);
        self.run(&full_args).await
    }
}

/// Parse clone args, extracting the target directory if present.
fn parse_clone_args(extra_args: &[&str]) -> (Vec<String>, String) {
    let mut args: Vec<String> = extra_args.iter().map(|s| (*s).to_string()).collect();
    let mut target = String::new();

    if let Some(first) = args.first()
        && !first.starts_with('-')
    {
        target = args.remove(0);
    }

    (args, target)
}

/// Parse branch config output lines.
fn parse_branch_config(output: &str) -> BranchConfig {
    let mut cfg = BranchConfig::default();

    for line in output.lines() {
        let Some((key_part, value)) = line.split_once(' ') else {
            continue;
        };
        let keys: Vec<&str> = key_part.split('.').collect();
        let Some(last_key) = keys.last() else {
            continue;
        };

        match *last_key {
            "remote" => {
                let (url, name) = parse_remote_url_or_name(value);
                cfg.remote_url = url;
                cfg.remote_name = name;
            }
            "pushremote" => {
                let (url, name) = parse_remote_url_or_name(value);
                cfg.push_remote_url = url;
                cfg.push_remote_name = name;
            }
            "merge" => cfg.merge_ref = value.to_string(),
            key if key == MERGE_BASE_CONFIG => cfg.merge_base = value.to_string(),
            _ => {}
        }
    }

    cfg
}

/// Parse a value that could be a remote URL or a remote name.
fn parse_remote_url_or_name(value: &str) -> (String, String) {
    if value.contains(':') {
        // Looks like a URL
        (value.to_string(), String::new())
    } else if !is_filesystem_path(value) {
        // Looks like a remote name
        (String::new(), value.to_string())
    } else {
        (String::new(), String::new())
    }
}

fn is_filesystem_path(p: &str) -> bool {
    p == "." || p.starts_with("./") || p.starts_with('/')
}

/// Get the first line of output.
fn first_line(output: &str) -> &str {
    output.lines().next().unwrap_or("")
}

/// Escape glob metacharacters in a string.
fn escape_glob(s: &str) -> String {
    s.replace('*', r"\*")
        .replace('?', r"\?")
        .replace('[', r"\[")
        .replace(']', r"\]")
        .replace('{', r"\{")
        .replace('}', r"\}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_parse_remote_tracking_ref() {
        let r = RemoteTrackingRef::parse("refs/remotes/origin/main").unwrap();
        assert_eq!(r.remote, "origin");
        assert_eq!(r.branch, "main");
    }

    #[test]
    fn test_should_parse_remote_tracking_ref_with_slashes() {
        let r = RemoteTrackingRef::parse("refs/remotes/origin/feature/branch").unwrap();
        assert_eq!(r.remote, "origin");
        assert_eq!(r.branch, "feature/branch");
    }

    #[test]
    fn test_should_reject_invalid_remote_tracking_ref() {
        assert!(RemoteTrackingRef::parse("refs/heads/main").is_err());
        assert!(RemoteTrackingRef::parse("refs/remotes/noslash").is_err());
    }

    #[test]
    fn test_should_display_remote_tracking_ref() {
        let r = RemoteTrackingRef {
            remote: "origin".to_string(),
            branch: "main".to_string(),
        };
        assert_eq!(r.to_string(), "refs/remotes/origin/main");
    }

    #[test]
    fn test_should_parse_push_default() {
        assert_eq!(PushDefault::parse("simple").unwrap(), PushDefault::Simple);
        assert_eq!(PushDefault::parse("current").unwrap(), PushDefault::Current);
        assert!(PushDefault::parse("invalid").is_err());
    }

    #[test]
    fn test_should_parse_branch_config() {
        let output = "branch.main.remote origin\n\
                       branch.main.merge refs/heads/main\n\
                       branch.main.gh-merge-base develop";
        let cfg = parse_branch_config(output);
        assert_eq!(cfg.remote_name, "origin");
        assert_eq!(cfg.merge_ref, "refs/heads/main");
        assert_eq!(cfg.merge_base, "develop");
    }

    #[test]
    fn test_should_parse_branch_config_with_url_remote() {
        let output = "branch.main.remote https://github.com/cli/cli.git";
        let cfg = parse_branch_config(output);
        assert_eq!(cfg.remote_url, "https://github.com/cli/cli.git");
        assert!(cfg.remote_name.is_empty());
    }

    #[test]
    fn test_should_parse_clone_args_with_target() {
        let (args, target) = parse_clone_args(&["my-dir", "--depth", "1"]);
        assert_eq!(target, "my-dir");
        assert_eq!(args, vec!["--depth", "1"]);
    }

    #[test]
    fn test_should_parse_clone_args_without_target() {
        let (args, target) = parse_clone_args(&["--depth", "1"]);
        assert!(target.is_empty());
        assert_eq!(args, vec!["--depth", "1"]);
    }

    #[test]
    fn test_should_escape_glob() {
        assert_eq!(escape_glob("foo*bar"), r"foo\*bar");
        assert_eq!(escape_glob("no[glob]"), r"no\[glob\]");
    }

    #[test]
    fn test_should_create_credential_pattern_from_host() {
        let pattern = CredentialPattern::from_host("github.com");
        match pattern {
            CredentialPattern::Host(p) => {
                assert!(p.contains("github.com"));
            }
            CredentialPattern::AllMatching => panic!("expected Host pattern"),
        }
    }

    // --- PushDefault ---

    #[test]
    fn test_should_parse_all_push_default_values() {
        assert_eq!(PushDefault::parse("nothing").unwrap(), PushDefault::Nothing);
        assert_eq!(PushDefault::parse("current").unwrap(), PushDefault::Current);
        assert_eq!(
            PushDefault::parse("upstream").unwrap(),
            PushDefault::Upstream
        );
        assert_eq!(
            PushDefault::parse("tracking").unwrap(),
            PushDefault::Tracking
        );
        assert_eq!(PushDefault::parse("simple").unwrap(), PushDefault::Simple);
        assert_eq!(
            PushDefault::parse("matching").unwrap(),
            PushDefault::Matching
        );
    }

    #[test]
    fn test_should_reject_unknown_push_default() {
        assert!(PushDefault::parse("").is_err());
        assert!(PushDefault::parse("invalid").is_err());
        assert!(PushDefault::parse("Simple").is_err()); // case-sensitive
    }

    // --- RemoteTrackingRef ---

    #[test]
    fn test_should_roundtrip_remote_tracking_ref() {
        let r = RemoteTrackingRef {
            remote: "upstream".to_string(),
            branch: "develop".to_string(),
        };
        let s = r.to_string();
        let parsed = RemoteTrackingRef::parse(&s).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn test_should_reject_empty_remote_tracking_ref() {
        assert!(RemoteTrackingRef::parse("").is_err());
    }

    #[test]
    fn test_should_reject_refs_heads_as_remote_tracking() {
        assert!(RemoteTrackingRef::parse("refs/heads/main").is_err());
    }

    // --- parse_branch_config ---

    #[test]
    fn test_should_parse_branch_config_with_pushremote() {
        let output = "branch.feat.pushremote upstream";
        let cfg = parse_branch_config(output);
        assert_eq!(cfg.push_remote_name, "upstream");
        assert!(cfg.push_remote_url.is_empty());
    }

    #[test]
    fn test_should_parse_branch_config_with_pushremote_url() {
        let output = "branch.feat.pushremote git@github.com:org/repo.git";
        let cfg = parse_branch_config(output);
        assert_eq!(cfg.push_remote_url, "git@github.com:org/repo.git");
        assert!(cfg.push_remote_name.is_empty());
    }

    #[test]
    fn test_should_return_default_for_empty_branch_config() {
        let cfg = parse_branch_config("");
        assert_eq!(cfg, BranchConfig::default());
    }

    #[test]
    fn test_should_skip_malformed_config_lines() {
        let output = "no_space_separator\nbranch.main.remote origin";
        let cfg = parse_branch_config(output);
        assert_eq!(cfg.remote_name, "origin");
    }

    // --- parse_clone_args ---

    #[test]
    fn test_should_parse_empty_clone_args() {
        let (args, target) = parse_clone_args(&[]);
        assert!(target.is_empty());
        assert!(args.is_empty());
    }

    #[test]
    fn test_should_parse_clone_args_with_only_flags() {
        let (args, target) = parse_clone_args(&["--bare", "--mirror"]);
        assert!(target.is_empty());
        assert_eq!(args, vec!["--bare", "--mirror"]);
    }

    // --- CredentialPattern ---

    #[test]
    fn test_should_create_credential_pattern_from_git_url() {
        let pattern = CredentialPattern::from_git_url("https://github.com/cli/cli.git").unwrap();
        match pattern {
            CredentialPattern::Host(p) => {
                assert!(p.contains("github.com"));
            }
            CredentialPattern::AllMatching => panic!("expected Host pattern"),
        }
    }

    #[test]
    fn test_should_reject_invalid_git_url_for_credential() {
        // A completely invalid URL
        assert!(CredentialPattern::from_git_url("not a url at all").is_err());
    }

    // --- first_line ---

    #[test]
    fn test_should_return_first_line_from_multiline() {
        assert_eq!(first_line("first\nsecond\nthird"), "first");
    }

    #[test]
    fn test_should_return_empty_for_empty_string() {
        assert_eq!(first_line(""), "");
    }

    #[test]
    fn test_should_return_single_line() {
        assert_eq!(first_line("only line"), "only line");
    }

    // --- escape_glob ---

    #[test]
    fn test_should_escape_all_glob_metacharacters() {
        assert_eq!(escape_glob("a*b?c[d]e{f}g"), r"a\*b\?c\[d\]e\{f\}g");
    }

    #[test]
    fn test_should_not_escape_normal_characters() {
        assert_eq!(escape_glob("hello-world_123"), "hello-world_123");
    }

    // --- is_filesystem_path ---

    #[test]
    fn test_should_detect_filesystem_paths() {
        assert!(is_filesystem_path("."));
        assert!(is_filesystem_path("./relative"));
        assert!(is_filesystem_path("/absolute"));
        assert!(!is_filesystem_path("origin"));
        assert!(!is_filesystem_path("upstream"));
    }

    // --- parse_remote_url_or_name ---

    #[test]
    fn test_should_parse_remote_name() {
        let (url, name) = parse_remote_url_or_name("origin");
        assert!(url.is_empty());
        assert_eq!(name, "origin");
    }

    #[test]
    fn test_should_parse_remote_url_with_colon() {
        let (url, name) = parse_remote_url_or_name("git@github.com:cli/cli");
        assert_eq!(url, "git@github.com:cli/cli");
        assert!(name.is_empty());
    }

    #[test]
    fn test_should_return_empty_for_filesystem_path() {
        let (url, name) = parse_remote_url_or_name("./local");
        assert!(url.is_empty());
        assert!(name.is_empty());
    }
}
