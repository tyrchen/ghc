//! User-related API queries.

use serde::{Deserialize, Serialize};

/// Current authenticated user info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewer {
    /// Login name.
    pub login: String,
    /// Display name.
    pub name: Option<String>,
}

/// GraphQL query for the authenticated user.
pub const VIEWER_QUERY: &str = r"
query Viewer {
  viewer {
    login
    name
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_deserialize_viewer() {
        let json = r#"{"login": "octocat", "name": "The Octocat"}"#;
        let viewer: Viewer = serde_json::from_str(json).unwrap();
        assert_eq!(viewer.login, "octocat");
        assert_eq!(viewer.name, Some("The Octocat".to_string()));
    }

    #[test]
    fn test_should_deserialize_viewer_without_name() {
        let json = r#"{"login": "octocat"}"#;
        let viewer: Viewer = serde_json::from_str(json).unwrap();
        assert_eq!(viewer.login, "octocat");
        assert!(viewer.name.is_none());
    }

    #[test]
    fn test_should_contain_viewer_query() {
        assert!(VIEWER_QUERY.contains("viewer"));
        assert!(VIEWER_QUERY.contains("login"));
    }
}
