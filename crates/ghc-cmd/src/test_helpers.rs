//! Shared test utilities for command testing.
//!
//! Provides factory builders, wiremock helpers, and assertion utilities
//! for testing command implementations in isolation.

use std::sync::Arc;

use ghc_core::browser::StubBrowser;
use ghc_core::config::MemoryConfig;
use ghc_core::iostreams::TestOutput;
use ghc_core::prompter::StubPrompter;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::factory::Factory;

/// A fully-configured test harness with factory, output capture, and mock server.
#[derive(Debug)]
pub struct TestHarness {
    /// The factory configured for testing.
    pub factory: Factory,
    /// Captured stdout/stderr output.
    pub output: TestOutput,
    /// Wiremock mock server for API requests.
    pub server: MockServer,
    /// Stub browser for verifying opened URLs.
    pub browser: Arc<StubBrowser>,
    /// Stub prompter for providing test answers.
    pub prompter: Arc<StubPrompter>,
}

impl TestHarness {
    /// Create a new test harness with a wiremock server and default config.
    ///
    /// The factory is pre-configured to route all API requests to the mock server,
    /// with a test auth token, stub browser, and stub prompter.
    pub async fn new() -> Self {
        Self::with_config(MemoryConfig::new().with_host(
            "github.com",
            "testuser",
            "ghp_test_token_123",
        ))
        .await
    }

    /// Create a test harness with a custom `MemoryConfig`.
    pub async fn with_config(config: MemoryConfig) -> Self {
        let server = MockServer::start().await;
        let (factory, output) = Factory::test();
        let (factory, browser) = factory.with_stub_browser();
        let (factory, prompter) = factory.with_stub_prompter();
        let factory = factory
            .with_http_client(reqwest::Client::new())
            .with_api_url(format!("{}/", server.uri()))
            .with_token("ghp_test_token_123")
            .with_config(Box::new(config));

        Self {
            factory,
            output,
            server,
            browser,
            prompter,
        }
    }

    /// Get captured stdout as a string.
    pub fn stdout(&self) -> String {
        self.output.stdout()
    }

    /// Get captured stderr as a string.
    pub fn stderr(&self) -> String {
        self.output.stderr()
    }

    /// Get URLs opened in the stub browser.
    pub fn opened_urls(&self) -> Vec<String> {
        self.browser
            .urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

// --- Wiremock helpers ---

/// Mount a GraphQL response mock that matches a query substring.
///
/// # Example
///
/// ```ignore
/// mock_graphql(&harness.server, "repository", json!({
///     "data": { "repository": { "issues": { "nodes": [] } } }
/// })).await;
/// ```
pub async fn mock_graphql(
    server: &MockServer,
    query_contains: &str,
    response_body: serde_json::Value,
) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(query_contains))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(server)
        .await;
}

/// Mount a REST GET response mock for a specific path.
pub async fn mock_rest_get(server: &MockServer, url_path: &str, response_body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .and(header("Authorization", "token ghp_test_token_123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(server)
        .await;
}

/// Mount a REST POST response mock for a specific path.
pub async fn mock_rest_post(
    server: &MockServer,
    url_path: &str,
    status: u16,
    response_body: serde_json::Value,
) {
    Mock::given(method("POST"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(response_body))
        .mount(server)
        .await;
}

/// Mount a REST GET response mock that returns a specific status code with a JSON body.
pub async fn mock_rest_get_status(
    server: &MockServer,
    url_path: &str,
    status: u16,
    response_body: serde_json::Value,
) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .and(header("Authorization", "token ghp_test_token_123"))
        .respond_with(ResponseTemplate::new(status).set_body_json(response_body))
        .mount(server)
        .await;
}

/// Mount a REST DELETE response mock for a specific path.
pub async fn mock_rest_delete(server: &MockServer, url_path: &str, status: u16) {
    Mock::given(method("DELETE"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

/// Mount a REST PATCH response mock for a specific path.
pub async fn mock_rest_patch(
    server: &MockServer,
    url_path: &str,
    status: u16,
    response_body: serde_json::Value,
) {
    Mock::given(method("PATCH"))
        .and(path(url_path))
        .respond_with(ResponseTemplate::new(status).set_body_json(response_body))
        .mount(server)
        .await;
}

// --- Common GraphQL response fixtures ---

/// Build a standard GraphQL issue list response.
pub fn graphql_issue_list_response(issues: &[serde_json::Value]) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "repository": {
                "issues": {
                    "nodes": issues,
                    "totalCount": issues.len(),
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }
    })
}

/// Build a single issue fixture.
pub fn issue_fixture(number: i64, title: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "state": state,
        "url": format!("https://github.com/owner/repo/issues/{number}"),
        "author": { "login": "testuser" },
        "labels": { "nodes": [] },
        "comments": { "totalCount": 0 },
        "createdAt": "2024-01-15T10:00:00Z",
        "updatedAt": "2024-01-15T10:00:00Z"
    })
}

/// Build a standard GraphQL PR list response.
pub fn graphql_pr_list_response(prs: &[serde_json::Value]) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "repository": {
                "pullRequests": {
                    "nodes": prs,
                    "totalCount": prs.len(),
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        }
    })
}

/// Build a single PR fixture.
pub fn pr_fixture(number: i64, title: &str, state: &str) -> serde_json::Value {
    serde_json::json!({
        "number": number,
        "title": title,
        "state": state,
        "url": format!("https://github.com/owner/repo/pull/{number}"),
        "headRefName": "feature-branch",
        "baseRefName": "main",
        "author": { "login": "testuser" },
        "labels": { "nodes": [] },
        "reviews": { "nodes": [] },
        "statusCheckRollup": null,
        "isDraft": false,
        "mergeable": "MERGEABLE",
        "additions": 10,
        "deletions": 5,
        "createdAt": "2024-01-15T10:00:00Z",
        "updatedAt": "2024-01-15T10:00:00Z"
    })
}

/// Build a standard GraphQL repository response.
pub fn graphql_repo_response(owner: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "data": {
            "repository": {
                "name": name,
                "owner": { "login": owner },
                "description": "A test repository",
                "url": format!("https://github.com/{owner}/{name}"),
                "homepageUrl": null,
                "isPrivate": false,
                "isFork": false,
                "isArchived": false,
                "defaultBranchRef": { "name": "main" },
                "stargazerCount": 42,
                "forkCount": 5,
                "watchers": { "totalCount": 10 },
                "issues": { "totalCount": 3 },
                "pullRequests": { "totalCount": 1 },
                "licenseInfo": { "name": "MIT License" },
                "primaryLanguage": { "name": "Rust" }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_should_create_test_harness() {
        let h = TestHarness::new().await;
        assert!(h.stdout().is_empty());
        assert!(h.stderr().is_empty());
        assert!(h.opened_urls().is_empty());
    }

    #[tokio::test]
    async fn test_should_create_harness_with_custom_config() {
        let config = MemoryConfig::new()
            .with_host("github.com", "user1", "token1")
            .with_host("ghe.corp.com", "user2", "token2");
        let h = TestHarness::with_config(config).await;
        assert!(h.stdout().is_empty());
    }

    #[tokio::test]
    async fn test_should_capture_output_through_factory() {
        let h = TestHarness::new().await;
        h.factory.io.println_out("hello from test");
        assert_eq!(h.stdout(), "hello from test\n");
    }

    #[tokio::test]
    async fn test_should_record_browser_opens() {
        let h = TestHarness::new().await;
        h.factory.browser().open("https://example.com").unwrap();
        let urls = h.opened_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com");
    }

    #[tokio::test]
    async fn test_should_mount_graphql_mock() {
        let h = TestHarness::new().await;
        let response = graphql_issue_list_response(&[issue_fixture(1, "Bug", "OPEN")]);
        mock_graphql(&h.server, "repository", response).await;

        let client = h.factory.api_client("github.com").unwrap();
        let result: serde_json::Value = client
            .graphql(
                "query { repository(owner: $owner, name: $name) { issues { nodes { number } } } }",
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        let nodes = result
            .pointer("/repository/issues/nodes")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["number"], 1);
    }
}
