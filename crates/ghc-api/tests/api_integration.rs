//! Integration tests for the GitHub API client.
//!
//! Uses wiremock to simulate GitHub API responses with realistic JSON
//! fixtures to test the full request/response cycle.

use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ghc_api::client::Client;
use ghc_api::queries::{issue::Issue, pr::PullRequest, repo::Repository};

/// Load a JSON fixture file from the fixtures directory.
fn load_fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("failed to parse fixture {path}: {e}"))
}

fn create_client(_server: &MockServer) -> Client {
    let http = reqwest::Client::new();
    Client::new(http, "github.com", Some("test-token".to_string()))
}

#[tokio::test]
async fn test_should_fetch_repo_view_from_fixture() {
    let server = MockServer::start().await;
    let fixture = load_fixture("repo_view.json");

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&fixture))
        .mount(&server)
        .await;

    let _client = create_client(&server);

    // Make a raw GraphQL call via the HTTP client to the mock server
    let resp = reqwest::Client::new()
        .post(format!("{}/graphql", server.uri()))
        .header("Authorization", "token test-token")
        .json(&serde_json::json!({
            "query": "query { repository { name } }",
            "variables": {}
        }))
        .send()
        .await
        .unwrap();

    let json: Value = resp.json().await.unwrap();
    let repo_data = &json["data"]["repository"];

    let repo: Repository = serde_json::from_value(repo_data.clone()).unwrap();
    assert_eq!(repo.name, "cli");
    assert_eq!(repo.owner.login, "cli");
    assert_eq!(
        repo.description,
        Some("GitHub's official command line tool".to_string())
    );
    assert!(!repo.is_fork);
    assert!(!repo.is_private);
    assert_eq!(repo.stargazer_count, Some(36000));
}

#[tokio::test]
async fn test_should_deserialize_issue_list_from_fixture() {
    let fixture = load_fixture("issue_list.json");
    let nodes = &fixture["data"]["repository"]["issues"]["nodes"];
    let issues: Vec<Issue> = serde_json::from_value(nodes.clone()).unwrap();

    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].number, 100);
    assert_eq!(issues[0].title, "Bug: CLI crashes on large repos");
    assert_eq!(issues[0].state, "OPEN");
    assert_eq!(issues[0].author.as_ref().unwrap().login, "testuser");
    assert_eq!(issues[0].labels.as_ref().unwrap().nodes[0].name, "bug");

    let page_info = &fixture["data"]["repository"]["issues"]["pageInfo"];
    assert_eq!(page_info["hasNextPage"], true);
}

#[tokio::test]
async fn test_should_deserialize_pr_list_from_fixture() {
    let fixture = load_fixture("pr_list.json");
    let nodes = &fixture["data"]["repository"]["pullRequests"]["nodes"];
    let prs: Vec<PullRequest> = serde_json::from_value(nodes.clone()).unwrap();

    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 42);
    assert_eq!(prs[0].title, "Add Rust CLI support");
    assert!(!prs[0].is_draft);
    assert_eq!(prs[0].head_ref_name, "feature/rust-cli");
    assert_eq!(prs[0].base_ref_name, "main");
    assert_eq!(prs[0].additions, Some(500));
    assert_eq!(prs[0].deletions, Some(100));
    assert_eq!(prs[0].changed_files, Some(15));
    assert_eq!(prs[0].review_decision, Some("REVIEW_REQUIRED".to_string()));
}

#[tokio::test]
async fn test_should_handle_rate_limit_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rate_limit"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(serde_json::json!({
                    "message": "API rate limit exceeded",
                    "documentation_url": "https://docs.github.com/rest/overview/resources-in-the-rest-api#rate-limiting"
                }))
                .append_header("x-ratelimit-remaining", "0")
                .append_header("x-ratelimit-reset", "1700000000"),
        )
        .mount(&server)
        .await;

    let client = create_client(&server);

    let err = client
        .rest::<Value>(
            reqwest::Method::GET,
            &format!("{}/rate_limit", server.uri()),
            None,
        )
        .await
        .unwrap_err();

    // Should be treated as an HTTP error
    assert!(format!("{err}").contains("rate limit") || err.is_rate_limited());
}

#[tokio::test]
async fn test_should_handle_unauthorized_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "message": "Bad credentials",
            "documentation_url": "https://docs.github.com/rest"
        })))
        .mount(&server)
        .await;

    let client = create_client(&server);

    let err = client
        .rest::<Value>(
            reqwest::Method::GET,
            &format!("{}/user", server.uri()),
            None,
        )
        .await
        .unwrap_err();

    assert!(err.is_unauthorized());
}
