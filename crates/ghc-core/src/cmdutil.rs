//! Command utility types and helpers.
//!
//! Maps from Go's `pkg/cmdutil` package.

/// Error indicating user cancelled an operation.
#[derive(Debug, thiserror::Error)]
#[error("user cancelled")]
pub struct CancelError;

/// Error indicating a flag parsing issue.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FlagError(pub String);

/// Error indicating no results were found (exit 0, not a failure).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NoResultsError(pub String);

/// Silent error - triggers exit 1 without message.
#[derive(Debug, thiserror::Error)]
#[error("")]
pub struct SilentError;

/// Pending error - triggers exit 8.
#[derive(Debug, thiserror::Error)]
#[error("")]
pub struct PendingError;

/// Auth error - triggers exit 4.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AuthError(pub String);

/// Check if an error represents a user cancellation.
pub fn is_user_cancellation(err: &anyhow::Error) -> bool {
    err.downcast_ref::<CancelError>().is_some()
}

/// Determine the editor to use, checking config, env vars, and defaults.
pub fn determine_editor<C: crate::config::Config + ?Sized>(config: &C, hostname: &str) -> String {
    // Check GH_EDITOR env var
    if let Ok(editor) = std::env::var("GH_EDITOR")
        && !editor.is_empty()
    {
        return editor;
    }

    // Check config
    if let Some(editor) = config.editor(hostname)
        && !editor.is_empty()
    {
        return editor;
    }

    // Check VISUAL
    if let Ok(editor) = std::env::var("VISUAL")
        && !editor.is_empty()
    {
        return editor;
    }

    // Check EDITOR
    if let Ok(editor) = std::env::var("EDITOR")
        && !editor.is_empty()
    {
        return editor;
    }

    // Default
    "nano".to_string()
}

/// Check if the user is authenticated for any host.
pub fn check_auth<C: crate::config::Config + ?Sized>(config: &C) -> bool {
    // Check environment token
    if std::env::var("GH_TOKEN").is_ok() || std::env::var("GITHUB_TOKEN").is_ok() {
        return true;
    }

    // Check config hosts
    !config.hosts().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, FileConfig};

    #[test]
    fn test_should_display_cancel_error() {
        let err = CancelError;
        assert_eq!(err.to_string(), "user cancelled");
    }

    #[test]
    fn test_should_display_flag_error() {
        let err = FlagError("invalid flag --bad".to_string());
        assert_eq!(err.to_string(), "invalid flag --bad");
    }

    #[test]
    fn test_should_display_no_results_error() {
        let err = NoResultsError("no issues match filters".to_string());
        assert_eq!(err.to_string(), "no issues match filters");
    }

    #[test]
    fn test_should_display_silent_error() {
        let err = SilentError;
        assert_eq!(err.to_string(), "");
    }

    #[test]
    fn test_should_display_pending_error() {
        let err = PendingError;
        assert_eq!(err.to_string(), "");
    }

    #[test]
    fn test_should_display_auth_error() {
        let err = AuthError("not logged in".to_string());
        assert_eq!(err.to_string(), "not logged in");
    }

    #[test]
    fn test_should_detect_user_cancellation() {
        let err: anyhow::Error = CancelError.into();
        assert!(is_user_cancellation(&err));
    }

    #[test]
    fn test_should_not_detect_non_cancel_as_cancellation() {
        let err = anyhow::anyhow!("some other error");
        assert!(!is_user_cancellation(&err));
    }

    #[test]
    fn test_should_determine_editor_from_config() {
        let _guards = [EnvGuard::unset("GH_EDITOR")];
        let mut cfg = FileConfig::empty();
        cfg.set("", "editor", "code").unwrap();
        let editor = determine_editor(&cfg, "");
        assert_eq!(editor, "code");
    }

    #[test]
    fn test_should_fallback_to_nano() {
        let _guards = [
            EnvGuard::unset("GH_EDITOR"),
            EnvGuard::unset("VISUAL"),
            EnvGuard::unset("EDITOR"),
        ];
        let cfg = FileConfig::empty();
        let editor = determine_editor(&cfg, "");
        assert_eq!(editor, "nano");
    }

    #[test]
    fn test_should_check_auth_returns_false_for_empty() {
        let _guards = [EnvGuard::unset("GH_TOKEN"), EnvGuard::unset("GITHUB_TOKEN")];
        let cfg = FileConfig::empty();
        assert!(!check_auth(&cfg));
    }

    #[test]
    fn test_should_check_auth_returns_true_with_hosts() {
        let _guards = [EnvGuard::unset("GH_TOKEN"), EnvGuard::unset("GITHUB_TOKEN")];
        let mut cfg = FileConfig::empty();
        cfg.set("github.com", "oauth_token", "token").unwrap();
        assert!(check_auth(&cfg));
    }

    struct EnvGuard {
        key: String,
        original: Option<String>,
    }

    impl EnvGuard {
        fn unset(key: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: Tests are run single-threaded with --test-threads=1
            // when env vars are involved, avoiding data races.
            unsafe { std::env::remove_var(key) };
            Self {
                key: key.to_string(),
                original,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: See EnvGuard::unset
                Some(val) => unsafe { std::env::set_var(&self.key, val) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }
}
