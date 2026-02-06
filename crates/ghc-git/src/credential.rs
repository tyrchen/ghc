//! Git credential protocol handler for `ghc auth git-credential`.
//!
//! Implements the git credential helper protocol to provide GitHub
//! authentication tokens to git operations automatically.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use tracing::debug;

use ghc_core::config::AuthConfig;
use ghc_core::instance;

/// Handle a git credential helper request.
///
/// Reads the credential request from stdin, looks up the appropriate
/// token from the authentication config, and writes the response to stdout.
///
/// # Errors
///
/// Returns an error if I/O fails or the request format is invalid.
pub fn handle_credential_request<R: BufRead, W: Write>(
    operation: &str,
    auth: &dyn AuthConfig,
    input: &mut R,
    output: &mut W,
) -> anyhow::Result<()> {
    match operation {
        "get" => handle_get(auth, input, output),
        "store" | "erase" => {
            // We don't need to handle store/erase since ghc manages tokens
            debug!("ignoring credential {operation} request");
            Ok(())
        }
        _ => {
            debug!("unknown credential operation: {operation}");
            Ok(())
        }
    }
}

/// Handle a `get` credential request.
fn handle_get<R: BufRead, W: Write>(
    auth: &dyn AuthConfig,
    input: &mut R,
    output: &mut W,
) -> anyhow::Result<()> {
    let fields = parse_credential_input(input)?;

    let protocol = fields.get("protocol").map_or("", String::as_str);
    let host = fields.get("host").map_or("", String::as_str);

    // Only handle HTTPS requests
    if protocol != "https" {
        debug!("skipping non-https credential request for protocol={protocol}");
        return Ok(());
    }

    if host.is_empty() {
        debug!("skipping credential request with no host");
        return Ok(());
    }

    // Normalize the hostname (strip port if present)
    let hostname = host.split(':').next().unwrap_or(host);
    let normalized = instance::normalize_hostname(hostname);

    // Look up the token
    let Some((token, source)) = auth.active_token(&normalized) else {
        debug!("no token found for host={normalized}");
        return Ok(());
    };

    debug!("providing credential for host={normalized} from source={source}");

    writeln!(output, "protocol=https")?;
    writeln!(output, "host={host}")?;
    writeln!(output, "username=x-access-token")?;
    writeln!(output, "password={token}")?;
    writeln!(output)?;

    Ok(())
}

/// Parse git credential protocol input into key-value pairs.
fn parse_credential_input<R: BufRead>(input: &mut R) -> anyhow::Result<HashMap<String, String>> {
    let mut fields = HashMap::new();

    for line in input.lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        }
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    /// A simple mock AuthConfig for testing.
    #[derive(Debug)]
    struct MockAuth {
        tokens: HashMap<String, String>,
    }

    impl AuthConfig for MockAuth {
        fn active_token(&self, hostname: &str) -> Option<(String, String)> {
            self.tokens
                .get(hostname)
                .map(|t| (t.clone(), "config".to_string()))
        }

        fn active_user(&self, _hostname: &str) -> Option<String> {
            None
        }

        fn hosts(&self) -> Vec<String> {
            self.tokens.keys().cloned().collect()
        }

        fn login(
            &mut self,
            _hostname: &str,
            _username: &str,
            _token: &str,
            _git_protocol: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn users_for_host(&self, _hostname: &str) -> Vec<String> {
            Vec::new()
        }

        fn token_for_user(&self, hostname: &str, _username: &str) -> Option<(String, String)> {
            self.tokens
                .get(hostname)
                .map(|t| (t.clone(), "config".to_string()))
        }

        fn logout(&mut self, _hostname: &str, _username: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn switch_user(&mut self, _hostname: &str, _username: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_should_provide_credential_for_known_host() {
        let auth = MockAuth {
            tokens: HashMap::from([("github.com".to_string(), "ghp_test123".to_string())]),
        };

        let input = "protocol=https\nhost=github.com\n\n";
        let mut reader = io::Cursor::new(input.as_bytes());
        let mut output = Vec::new();

        handle_credential_request("get", &auth, &mut reader, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.contains("protocol=https"));
        assert!(result.contains("host=github.com"));
        assert!(result.contains("username=x-access-token"));
        assert!(result.contains("password=ghp_test123"));
    }

    #[test]
    fn test_should_skip_non_https_requests() {
        let auth = MockAuth {
            tokens: HashMap::from([("github.com".to_string(), "ghp_test123".to_string())]),
        };

        let input = "protocol=ssh\nhost=github.com\n\n";
        let mut reader = io::Cursor::new(input.as_bytes());
        let mut output = Vec::new();

        handle_credential_request("get", &auth, &mut reader, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_should_skip_unknown_host() {
        let auth = MockAuth {
            tokens: HashMap::new(),
        };

        let input = "protocol=https\nhost=gitlab.com\n\n";
        let mut reader = io::Cursor::new(input.as_bytes());
        let mut output = Vec::new();

        handle_credential_request("get", &auth, &mut reader, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_should_ignore_store_operation() {
        let auth = MockAuth {
            tokens: HashMap::new(),
        };

        let input = "";
        let mut reader = io::Cursor::new(input.as_bytes());
        let mut output = Vec::new();

        handle_credential_request("store", &auth, &mut reader, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn test_should_parse_credential_input() {
        let input = "protocol=https\nhost=github.com\nusername=user\n\n";
        let mut reader = io::Cursor::new(input.as_bytes());
        let fields = parse_credential_input(&mut reader).unwrap();

        assert_eq!(fields.get("protocol"), Some(&"https".to_string()));
        assert_eq!(fields.get("host"), Some(&"github.com".to_string()));
        assert_eq!(fields.get("username"), Some(&"user".to_string()));
    }
}
