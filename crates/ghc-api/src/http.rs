//! HTTP client construction with middleware.
//!
//! Maps from Go's `api/http_client.go`. Provides default headers,
//! User-Agent, Accept, and verbose-logging configuration.

use reqwest::header::{self, HeaderMap, HeaderValue};
use tracing::debug;

/// Options for constructing an HTTP client.
#[derive(Debug)]
pub struct HttpClientOptions {
    /// Application version for User-Agent.
    pub app_version: String,
    /// Whether to skip default auth headers.
    pub skip_default_headers: bool,
    /// Enable verbose HTTP logging.
    pub log_verbose: bool,
}

/// Build a reqwest client with default configuration.
///
/// # Errors
///
/// Returns an error if the client cannot be constructed.
pub fn build_client(opts: &HttpClientOptions) -> anyhow::Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    if !opts.skip_default_headers {
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_str(&format!("GHC CLI {}", opts.app_version))?,
        );
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
    }

    if opts.log_verbose {
        debug!("Building HTTP client with verbose logging");
    }

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;

    Ok(client)
}

/// Format an authorization header value from a token.
pub fn auth_header_value(token: &str) -> String {
    format!("token {token}")
}

/// Check if an environment token is overriding config-based auth.
///
/// Returns `Some(env_var_name)` if an environment variable is providing the token,
/// or `None` if the token comes from config.
pub fn auth_token_env_override() -> Option<&'static str> {
    if std::env::var("GH_TOKEN").is_ok() {
        Some("GH_TOKEN")
    } else if std::env::var("GITHUB_TOKEN").is_ok() {
        Some("GITHUB_TOKEN")
    } else {
        None
    }
}

/// Check whether the token for a hostname is writeable (not from env).
///
/// Returns `(env_var_name, false)` if the token comes from an environment variable,
/// or `("", true)` if it is writeable.
pub fn auth_token_writeable(source: &str) -> (String, bool) {
    if source.ends_with("_TOKEN") {
        (source.to_string(), false)
    } else {
        (source.to_string(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_build_client_with_defaults() {
        let opts = HttpClientOptions {
            app_version: "1.0.0".to_string(),
            skip_default_headers: false,
            log_verbose: false,
        };
        let client = build_client(&opts);
        assert!(client.is_ok());
    }

    #[test]
    fn test_should_build_client_skipping_headers() {
        let opts = HttpClientOptions {
            app_version: "1.0.0".to_string(),
            skip_default_headers: true,
            log_verbose: false,
        };
        let client = build_client(&opts);
        assert!(client.is_ok());
    }

    #[test]
    fn test_should_build_client_with_verbose() {
        let opts = HttpClientOptions {
            app_version: "1.0.0".to_string(),
            skip_default_headers: false,
            log_verbose: true,
        };
        let client = build_client(&opts);
        assert!(client.is_ok());
    }

    #[test]
    fn test_should_format_auth_header() {
        assert_eq!(auth_header_value("ghp_abc123"), "token ghp_abc123");
        assert_eq!(auth_header_value(""), "token ");
    }

    #[test]
    fn test_should_detect_writeable_token() {
        let (source, writeable) = auth_token_writeable("config");
        assert!(writeable);
        assert_eq!(source, "config");
    }

    #[test]
    fn test_should_detect_non_writeable_env_token() {
        let (source, writeable) = auth_token_writeable("GH_TOKEN");
        assert!(!writeable);
        assert_eq!(source, "GH_TOKEN");
    }

    #[test]
    fn test_should_detect_github_token_non_writeable() {
        let (source, writeable) = auth_token_writeable("GITHUB_TOKEN");
        assert!(!writeable);
        assert_eq!(source, "GITHUB_TOKEN");
    }
}
