//! Git-related error types.

/// Errors from git operations.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// Git command failed with an exit code.
    #[error("git {command} failed: {message}")]
    CommandFailed {
        /// The git subcommand that failed.
        command: String,
        /// Error message from stderr.
        message: String,
        /// Process exit code, if available.
        exit_code: Option<i32>,
    },

    /// Not inside a git repository.
    #[error("not a git repository (or any parent up to mount point /)")]
    NotARepository,

    /// Git binary not found.
    #[error("git executable not found in PATH")]
    NotFound,

    /// Not on any branch (detached HEAD).
    #[error("git: not on any branch")]
    NotOnAnyBranch,

    /// No commits found between two refs.
    #[error("could not find any commits between {base_ref} and {head_ref}")]
    NoCommits {
        /// Base reference.
        base_ref: String,
        /// Head reference.
        head_ref: String,
    },

    /// Invalid credential pattern.
    #[error("empty credential pattern is not allowed unless provided explicitly")]
    InvalidCredentialPattern,

    /// I/O error from subprocess.
    #[error("git IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl GitError {
    /// Get the exit code if this was a command failure.
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::CommandFailed { exit_code, .. } => *exit_code,
            _ => None,
        }
    }

    /// Check if this is an exit code 1 (typically "not found" for config).
    pub fn is_exit_code_1(&self) -> bool {
        self.exit_code() == Some(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_display_command_failed() {
        let err = GitError::CommandFailed {
            command: "checkout".to_string(),
            message: "pathspec 'missing' did not match".to_string(),
            exit_code: Some(1),
        };
        let msg = err.to_string();
        assert!(msg.contains("checkout"));
        assert!(msg.contains("pathspec"));
    }

    #[test]
    fn test_should_display_not_a_repository() {
        let err = GitError::NotARepository;
        assert!(err.to_string().contains("not a git repository"));
    }

    #[test]
    fn test_should_display_not_found() {
        let err = GitError::NotFound;
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_should_display_not_on_any_branch() {
        let err = GitError::NotOnAnyBranch;
        assert!(err.to_string().contains("not on any branch"));
    }

    #[test]
    fn test_should_display_no_commits() {
        let err = GitError::NoCommits {
            base_ref: "main".to_string(),
            head_ref: "feature".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("main"));
        assert!(msg.contains("feature"));
    }

    #[test]
    fn test_should_display_invalid_credential_pattern() {
        let err = GitError::InvalidCredentialPattern;
        assert!(err.to_string().contains("credential pattern"));
    }

    #[test]
    fn test_should_display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = GitError::Io(io_err);
        assert!(err.to_string().contains("access denied"));
    }

    #[test]
    fn test_should_return_exit_code() {
        let err = GitError::CommandFailed {
            command: "push".to_string(),
            message: "rejected".to_string(),
            exit_code: Some(128),
        };
        assert_eq!(err.exit_code(), Some(128));
    }

    #[test]
    fn test_should_return_none_exit_code_for_non_command_error() {
        let err = GitError::NotARepository;
        assert!(err.exit_code().is_none());
    }

    #[test]
    fn test_should_detect_exit_code_1() {
        let err = GitError::CommandFailed {
            command: "config".to_string(),
            message: String::new(),
            exit_code: Some(1),
        };
        assert!(err.is_exit_code_1());
    }

    #[test]
    fn test_should_not_detect_exit_code_1_for_other_codes() {
        let err = GitError::CommandFailed {
            command: "push".to_string(),
            message: String::new(),
            exit_code: Some(128),
        };
        assert!(!err.is_exit_code_1());
    }

    #[test]
    fn test_should_convert_io_error() {
        let io_err = std::io::Error::other("test");
        let git_err: GitError = io_err.into();
        assert!(matches!(git_err, GitError::Io(_)));
    }
}
