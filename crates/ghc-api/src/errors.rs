//! API error types.

use std::collections::HashMap;

/// HTTP API error with status code and message.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ApiError {
    /// HTTP error response.
    #[error("HTTP {status}: {message}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Error message from the API.
        message: String,
        /// Suggested OAuth scopes to fix the error.
        scopes_suggestion: Option<String>,
        /// Response headers.
        headers: HashMap<String, String>,
    },

    /// GraphQL errors returned in the response body.
    #[error("GraphQL: {0:?}")]
    GraphQL(Vec<GraphQLErrorEntry>),

    /// Authentication required.
    #[error("authentication required: try running `ghc auth login`")]
    AuthRequired,

    /// Token is missing required OAuth scopes.
    #[error("missing required scopes: {}", .0.join(", "))]
    MissingScopes(Vec<String>),

    /// Network/transport error.
    #[error(transparent)]
    Request(#[from] reqwest::Error),

    /// JSON parsing error.
    #[error("failed to parse API response: {0}")]
    JsonParse(#[from] serde_json::Error),
}

/// A single GraphQL error entry.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GraphQLErrorEntry {
    /// Error message.
    pub message: String,
    /// Error type (if provided).
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    /// Path to the field that caused the error.
    pub path: Option<Vec<serde_json::Value>>,
}

impl ApiError {
    /// Check if this is a 404 Not Found error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Http { status: 404, .. })
    }

    /// Check if this is a 401 Unauthorized error.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Http { status: 401, .. })
    }

    /// Check if this is a rate-limit (429) error.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Http { status: 429, .. })
    }

    /// Get scopes suggestion for permission errors.
    pub fn scopes_suggestion(&self) -> Option<&str> {
        match self {
            Self::Http {
                scopes_suggestion: Some(s),
                ..
            } => Some(s.as_str()),
            _ => None,
        }
    }

    /// Get the missing scopes if this is a `MissingScopes` error.
    pub fn missing_scopes(&self) -> Option<&[String]> {
        match self {
            Self::MissingScopes(scopes) => Some(scopes),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_error(status: u16, message: &str) -> ApiError {
        ApiError::Http {
            status,
            message: message.to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }
    }

    #[test]
    fn test_should_detect_not_found() {
        assert!(http_error(404, "not found").is_not_found());
        assert!(!http_error(200, "ok").is_not_found());
        assert!(!http_error(500, "server error").is_not_found());
    }

    #[test]
    fn test_should_detect_unauthorized() {
        assert!(http_error(401, "unauthorized").is_unauthorized());
        assert!(!http_error(403, "forbidden").is_unauthorized());
        assert!(!http_error(200, "ok").is_unauthorized());
    }

    #[test]
    fn test_should_detect_rate_limited() {
        assert!(http_error(429, "too many requests").is_rate_limited());
        assert!(!http_error(403, "forbidden").is_rate_limited());
    }

    #[test]
    fn test_should_display_http_error() {
        let err = http_error(403, "forbidden");
        assert_eq!(err.to_string(), "HTTP 403: forbidden");
    }

    #[test]
    fn test_should_display_graphql_error() {
        let entries = vec![GraphQLErrorEntry {
            message: "field not found".to_string(),
            error_type: Some("NOT_FOUND".to_string()),
            path: None,
        }];
        let err = ApiError::GraphQL(entries);
        let msg = err.to_string();
        assert!(msg.contains("field not found"));
    }

    #[test]
    fn test_should_display_auth_required() {
        let err = ApiError::AuthRequired;
        assert!(err.to_string().contains("authentication required"));
    }

    #[test]
    fn test_should_display_missing_scopes() {
        let err = ApiError::MissingScopes(vec!["repo".to_string(), "read:org".to_string()]);
        let msg = err.to_string();
        assert!(msg.contains("repo"));
        assert!(msg.contains("read:org"));
    }

    #[test]
    fn test_should_return_missing_scopes() {
        let err = ApiError::MissingScopes(vec!["repo".to_string()]);
        assert_eq!(
            err.missing_scopes(),
            Some(vec!["repo".to_string()].as_slice())
        );
    }

    #[test]
    fn test_should_return_none_missing_scopes_for_other_errors() {
        let err = http_error(403, "forbidden");
        assert!(err.missing_scopes().is_none());
    }

    #[test]
    fn test_should_return_scopes_suggestion() {
        let err = ApiError::Http {
            status: 403,
            message: "missing scopes".to_string(),
            scopes_suggestion: Some("repo, read:org".to_string()),
            headers: HashMap::new(),
        };
        assert_eq!(err.scopes_suggestion(), Some("repo, read:org"));
    }

    #[test]
    fn test_should_return_none_scopes_for_non_http() {
        let err = ApiError::AuthRequired;
        assert!(err.scopes_suggestion().is_none());
    }

    #[test]
    fn test_should_return_none_scopes_when_absent() {
        let err = http_error(403, "forbidden");
        assert!(err.scopes_suggestion().is_none());
    }

    #[test]
    fn test_should_deserialize_graphql_error_entry() {
        let json = r#"{"message": "test error", "type": "ERROR", "path": ["repository"]}"#;
        let entry: GraphQLErrorEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.message, "test error");
        assert_eq!(entry.error_type, Some("ERROR".to_string()));
        assert!(entry.path.is_some());
    }

    #[test]
    fn test_should_deserialize_graphql_error_entry_minimal() {
        let json = r#"{"message": "test error"}"#;
        let entry: GraphQLErrorEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.message, "test error");
        assert!(entry.error_type.is_none());
        assert!(entry.path.is_none());
    }
}
