//! Core error types for the GHC CLI.

/// Errors originating from core operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    /// Configuration file read/write error.
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// User cancelled an interactive prompt.
    #[error("prompt cancelled by user")]
    Cancelled,

    /// A required value was not found.
    #[error("{0}")]
    NotFound(String),

    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serde(String),
}

/// Configuration-specific errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// Failed to read config file.
    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        /// Path of the config file.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to write config file.
    #[error("failed to write config file {path}: {source}")]
    WriteFile {
        /// Path of the config file.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to parse config.
    #[error("failed to parse config: {0}")]
    Parse(String),

    /// Missing required configuration.
    #[error("missing required configuration: {0}")]
    Missing(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_display_config_error_missing() {
        let err = ConfigError::Missing("git_protocol".to_string());
        assert_eq!(
            err.to_string(),
            "missing required configuration: git_protocol",
        );
    }

    #[test]
    fn test_should_display_config_error_parse() {
        let err = ConfigError::Parse("invalid yaml".to_string());
        assert_eq!(err.to_string(), "failed to parse config: invalid yaml");
    }

    #[test]
    fn test_should_display_config_error_read_file() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = ConfigError::ReadFile {
            path: "/home/.config/gh/config.yml".to_string(),
            source: io_err,
        };
        let msg = err.to_string();
        assert!(msg.contains("/home/.config/gh/config.yml"));
        assert!(msg.contains("no such file"));
    }

    #[test]
    fn test_should_display_config_error_write_file() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let err = ConfigError::WriteFile {
            path: "/etc/gh/config.yml".to_string(),
            source: io_err,
        };
        let msg = err.to_string();
        assert!(msg.contains("/etc/gh/config.yml"));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn test_should_display_core_error_from_config() {
        let config_err = ConfigError::Missing("editor".to_string());
        let core_err = CoreError::Config(config_err);
        assert_eq!(
            core_err.to_string(),
            "configuration error: missing required configuration: editor",
        );
    }

    #[test]
    fn test_should_display_core_error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe");
        let core_err = CoreError::Io(io_err);
        assert!(core_err.to_string().contains("broken pipe"));
    }

    #[test]
    fn test_should_display_core_error_cancelled() {
        let err = CoreError::Cancelled;
        assert_eq!(err.to_string(), "prompt cancelled by user");
    }

    #[test]
    fn test_should_display_core_error_not_found() {
        let err = CoreError::NotFound("repo not found".to_string());
        assert_eq!(err.to_string(), "repo not found");
    }

    #[test]
    fn test_should_display_core_error_serde() {
        let err = CoreError::Serde("expected string, got number".to_string());
        assert!(err.to_string().contains("expected string"));
    }

    #[test]
    fn test_should_convert_config_error_to_core_error() {
        let config_err = ConfigError::Missing("token".to_string());
        let core_err: CoreError = config_err.into();
        assert!(matches!(core_err, CoreError::Config(_)));
    }

    #[test]
    fn test_should_convert_io_error_to_core_error() {
        let io_err = std::io::Error::other("test");
        let core_err: CoreError = io_err.into();
        assert!(matches!(core_err, CoreError::Io(_)));
    }
}
