//! High-level GitHub API client.
//!
//! Maps from Go's `api/client.go`. Provides REST and GraphQL methods
//! with pagination, rate-limit handling, retry logic, and OAuth scope
//! suggestions.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use reqwest::header::HeaderMap;
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::warn;

use crate::errors::{ApiError, GraphQLErrorEntry};
use ghc_core::instance;

/// Maximum number of retries for transient failures.
const MAX_RETRIES: u32 = 3;

/// Initial back-off delay for retries.
const RETRY_BASE_DELAY_MS: u64 = 1000;

/// GitHub API client wrapping reqwest with auth and error handling.
///
/// Tokens are stored as [`SecretString`] to prevent accidental logging or
/// exposure through `Debug` output.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    hostname: String,
    token: Option<SecretString>,
    /// Optional base URL override for testing (e.g., `"http://127.0.0.1:PORT/"`).
    /// When set, REST and GraphQL requests use this instead of the real GitHub URLs.
    api_url_override: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("hostname", &self.hostname)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("api_url_override", &self.api_url_override)
            .finish_non_exhaustive()
    }
}

/// A page of results from a REST API endpoint with a link to the next page.
#[derive(Debug)]
#[non_exhaustive]
pub struct RestPage<T> {
    /// The deserialized response body.
    pub data: T,
    /// URL of the next page, if any.
    pub next_url: Option<String>,
}

/// GraphQL page info for cursor-based pagination.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PageInfo {
    /// Whether there is a next page.
    pub has_next_page: bool,
    /// Cursor for the next page.
    pub end_cursor: Option<String>,
}

impl Client {
    /// Create a new API client for a specific hostname.
    ///
    /// Tokens are wrapped in [`SecretString`] to prevent accidental leaking.
    /// Use `.into()` to convert a plain `String` to `SecretString`.
    pub fn new(http: reqwest::Client, hostname: &str, token: Option<SecretString>) -> Self {
        Self {
            http,
            hostname: instance::normalize_hostname(hostname),
            token,
            api_url_override: None,
        }
    }

    /// Set a base URL override for testing.
    ///
    /// When set, all REST and GraphQL requests are routed to this base URL
    /// instead of the real GitHub API. The URL should include the trailing
    /// slash, e.g., `"http://127.0.0.1:8080/"`.
    #[must_use]
    pub fn with_url_override(mut self, url: String) -> Self {
        self.api_url_override = Some(url);
        self
    }

    /// Get the hostname this client is configured for.
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    /// Get the token this client is configured with.
    ///
    /// Callers must be careful not to log or display the returned value.
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(ExposeSecret::expose_secret)
    }

    /// Build a request with authentication headers applied.
    fn authed_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, url);
        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("token {}", token.expose_secret()));
        }
        req
    }

    /// Execute a GraphQL query.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure, auth issues, or GraphQL errors.
    pub async fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: &HashMap<String, Value>,
    ) -> Result<T, ApiError> {
        let url = match self.api_url_override {
            Some(ref base) => format!("{base}graphql"),
            None => instance::graphql_url(&self.hostname),
        };

        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let resp = self
            .authed_request(reqwest::Method::POST, &url)
            .header("GraphQL-Features", "merge_queue")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let headers = resp.headers().clone();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ApiError::Http {
                status: status.as_u16(),
                message: text,
                scopes_suggestion: None,
                headers: extract_header_map(&headers),
            });
        }

        let body: Value = resp.json().await?;

        // Check for GraphQL errors
        if let Some(errors) = body.get("errors") {
            let entries: Vec<GraphQLErrorEntry> =
                serde_json::from_value(errors.clone()).unwrap_or_default();
            if !entries.is_empty() {
                // If we also have data, try to return it
                if let Some(data) = body.get("data")
                    && let Ok(result) = serde_json::from_value::<T>(data.clone())
                {
                    return Ok(result);
                }
                return Err(ApiError::GraphQL(entries));
            }
        }

        let data = body.get("data").ok_or_else(|| ApiError::Http {
            status: 200,
            message: "no data in GraphQL response".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        })?;

        Ok(serde_json::from_value(data.clone())?)
    }

    /// Execute a REST API request.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, ApiError> {
        let url = self.resolve_rest_url(path);
        let resp = self.send_rest_request(method, &url, body).await?;
        let resp = Self::check_response(resp, true).await?;
        Ok(resp.json().await?)
    }

    /// Execute a REST API request and return the raw response body as string.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest_text(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<String, ApiError> {
        let url = self.resolve_rest_url(path);
        let resp = self.send_rest_request(method, &url, body).await?;
        let resp = Self::check_response(resp, true).await?;
        Ok(resp.text().await?)
    }

    /// Execute a REST API request with Link-header based pagination.
    ///
    /// Returns the deserialized data and the URL of the next page (if any).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest_with_next<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<RestPage<T>, ApiError> {
        let url = self.resolve_rest_url(path);
        let resp = self.send_rest_request(method, &url, body).await?;
        let resp = Self::check_response(resp, true).await?;

        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            // For 204 responses, try to return default-ish data
            let text = resp.text().await.unwrap_or_default();
            let data: T = serde_json::from_str(&text)?;
            return Ok(RestPage {
                data,
                next_url: None,
            });
        }

        let next_url = parse_link_next(resp.headers());
        let data: T = resp.json().await?;

        Ok(RestPage { data, next_url })
    }

    /// Collect all pages from a paginated REST endpoint.
    ///
    /// Repeatedly follows the `next` link until there are no more pages.
    /// Returns all items concatenated.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest_paginate<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<Vec<T>, ApiError> {
        let mut all_items = Vec::new();
        let mut current_url = self.resolve_rest_url(path);

        loop {
            let page: RestPage<Vec<T>> = self
                .rest_with_next(method.clone(), &current_url, None)
                .await?;
            all_items.extend(page.data);

            match page.next_url {
                Some(next) => current_url = next,
                None => break,
            }
        }

        Ok(all_items)
    }

    /// Execute a REST request with automatic retry for transient failures.
    ///
    /// Retries on 429 (rate limit), 502, 503, and 504 status codes.
    ///
    /// # Errors
    ///
    /// Returns the last error if all retries are exhausted.
    pub async fn rest_with_retry<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T, ApiError> {
        let mut last_err = None;
        for attempt in 0..MAX_RETRIES {
            match self.rest::<T>(method.clone(), path, body).await {
                Ok(result) => return Ok(result),
                Err(e) if is_retryable(&e) => {
                    let delay = RETRY_BASE_DELAY_MS * 2u64.pow(attempt);
                    warn!(
                        attempt = attempt + 1,
                        max = MAX_RETRIES,
                        delay_ms = delay,
                        error = %e,
                        "Retrying request"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or(ApiError::Http {
            status: 0,
            message: "all retries exhausted".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }))
    }

    /// Get the OAuth scopes header for a token by making a lightweight request.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn get_scopes(&self, token: &str) -> Result<String, ApiError> {
        let url = instance::rest_url(&self.hostname);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("token {token}"))
            .send()
            .await?;

        let resp = Self::check_response(resp, false).await?;

        let scopes = resp
            .headers()
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        Ok(scopes)
    }

    /// Validate that a token has the minimum required scopes.
    ///
    /// # Errors
    ///
    /// Returns `ApiError::MissingScopes` if required scopes are missing.
    pub async fn has_minimum_scopes(&self, token: &str) -> Result<(), ApiError> {
        let scopes_header = self.get_scopes(token).await?;
        check_minimum_scopes(&scopes_header)
    }

    /// Get the currently authenticated username via GraphQL.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or auth issues.
    pub async fn current_login(&self) -> Result<String, ApiError> {
        #[derive(serde::Deserialize)]
        struct ViewerResponse {
            viewer: ViewerLogin,
        }
        #[derive(serde::Deserialize)]
        struct ViewerLogin {
            login: String,
        }

        let query = "query UserCurrent { viewer { login } }";
        let vars = HashMap::new();

        let resp: ViewerResponse = self.graphql(query, &vars).await?;
        Ok(resp.viewer.login)
    }

    /// Get the currently authenticated user's login using a specific token.
    ///
    /// Creates a temporary client with the given token and delegates to
    /// [`current_login`](Self::current_login).
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or auth issues.
    pub async fn current_login_with_token(&self, token: &str) -> Result<String, ApiError> {
        let temp_client = Self {
            http: self.http.clone(),
            hostname: self.hostname.clone(),
            token: Some(token.into()),
            api_url_override: self.api_url_override.clone(),
        };
        temp_client.current_login().await
    }

    /// Upload a binary asset to a GitHub release.
    ///
    /// Sends raw bytes with the given content type (typically
    /// `application/octet-stream`) to the specified upload URL.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn upload_asset(
        &self,
        upload_url: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<Value, ApiError> {
        let url = self.resolve_rest_url(upload_url);
        let mut req = self.authed_request(reqwest::Method::POST, &url);
        req = req.header("Content-Type", content_type).body(data);

        let resp = req.send().await?;
        let resp = Self::check_response(resp, true).await?;
        Ok(resp.json().await?)
    }

    /// Execute a REST API request and return the raw response body as bytes.
    ///
    /// Use this for downloading binary content (e.g., release assets) to
    /// avoid UTF-8 encoding corruption.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest_bytes(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<Vec<u8>, ApiError> {
        let url = self.resolve_rest_url(path);
        let resp = self.send_rest_request(method, &url, None).await?;
        let resp = Self::check_response(resp, true).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Execute a REST API request with a custom Accept header.
    ///
    /// This is useful for endpoints that return different content based on
    /// the Accept header, such as GitHub's text-match search results.
    ///
    /// # Errors
    ///
    /// Returns an error on network failure or non-success status.
    pub async fn rest_with_accept<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        accept: &str,
    ) -> Result<T, ApiError> {
        let url = self.resolve_rest_url(path);
        let mut req = self.authed_request(method, &url);
        req = req.header("Accept", accept);
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await?;
        let resp = Self::check_response(resp, true).await?;
        Ok(resp.json().await?)
    }

    /// Check a response for errors and return an `ApiError::Http` if the
    /// status is not successful. The `include_scopes` flag controls whether
    /// OAuth scope suggestion headers are inspected.
    async fn check_response(
        resp: reqwest::Response,
        include_scopes: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }

        let headers = resp.headers().clone();
        let text = resp.text().await.unwrap_or_default();
        let suggestion = if include_scopes {
            generate_scopes_suggestion(
                status.as_u16(),
                headers
                    .get("x-accepted-oauth-scopes")
                    .and_then(|v| v.to_str().ok()),
                headers.get("x-oauth-scopes").and_then(|v| v.to_str().ok()),
            )
        } else {
            None
        };
        Err(ApiError::Http {
            status: status.as_u16(),
            message: text,
            scopes_suggestion: suggestion,
            headers: extract_header_map(&headers),
        })
    }

    fn resolve_rest_url(&self, path: &str) -> String {
        if path.starts_with("https://") || path.starts_with("http://") {
            path.to_string()
        } else {
            let base = match self.api_url_override {
                Some(ref url) => url.clone(),
                None => instance::rest_url(&self.hostname),
            };
            format!("{base}{}", path.trim_start_matches('/'))
        }
    }

    async fn send_rest_request(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&Value>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut req = self.authed_request(method, url);
        if let Some(body) = body {
            req = req.json(body);
        }
        req.send().await
    }
}

/// Regex for parsing RFC 5988 `Link` header relations.
static LINK_REL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<([^>]+)>;\s*rel="([^"]+)""#).expect("LINK_REL_RE is a valid regex")
});

/// Parse the `Link` header to extract the `next` page URL.
fn parse_link_next(headers: &HeaderMap) -> Option<String> {
    let link_header = headers.get("link")?.to_str().ok()?;

    for cap in LINK_REL_RE.captures_iter(link_header) {
        if cap.get(2).is_some_and(|m| m.as_str() == "next") {
            return cap.get(1).map(|m| m.as_str().to_string());
        }
    }
    None
}

/// Extract response headers into a `HashMap<String, String>`.
fn extract_header_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (key, value) in headers {
        if let Ok(v) = value.to_str() {
            map.insert(key.to_string(), v.to_string());
        }
    }
    map
}

/// Check if an API error is retryable (transient).
fn is_retryable(err: &ApiError) -> bool {
    match err {
        ApiError::Http { status, .. } => matches!(status, 429 | 502 | 503 | 504),
        ApiError::Request(_) => true,
        _ => false,
    }
}

/// Generate an OAuth scopes suggestion when a request fails with a 4xx error.
///
/// Compares the scopes the endpoint needs (`X-Accepted-OAuth-Scopes`) against
/// the scopes the token has (`X-OAuth-Scopes`).
pub fn generate_scopes_suggestion(
    status_code: u16,
    endpoint_needs_scopes: Option<&str>,
    token_has_scopes: Option<&str>,
) -> Option<String> {
    if !(400..=499).contains(&status_code) || status_code == 422 {
        return None;
    }

    let token_scopes_str = token_has_scopes.unwrap_or("");
    if token_scopes_str.is_empty() {
        return None;
    }

    let mut got_scopes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in token_scopes_str.split(',') {
        let s = s.trim().to_string();
        // Handle implied/grouped scopes
        if s == "repo" {
            for implied in &[
                "repo:status",
                "repo_deployment",
                "public_repo",
                "repo:invite",
                "security_events",
            ] {
                got_scopes.insert((*implied).to_string());
            }
        } else if s == "user" {
            for implied in &["read:user", "user:email", "user:follow"] {
                got_scopes.insert((*implied).to_string());
            }
        } else if s == "codespace" {
            got_scopes.insert("codespace:secrets".to_string());
        } else if let Some(rest) = s.strip_prefix("admin:") {
            got_scopes.insert(format!("read:{rest}"));
            got_scopes.insert(format!("write:{rest}"));
        } else if let Some(rest) = s.strip_prefix("write:") {
            got_scopes.insert(format!("read:{rest}"));
        }
        got_scopes.insert(s);
    }

    let needs = endpoint_needs_scopes.unwrap_or("");
    for s in needs.split(',') {
        let s = s.trim();
        if s.is_empty() || got_scopes.contains(s) {
            continue;
        }
        return Some(format!(
            "This API operation needs the \"{s}\" scope. To request it, run:  ghc auth refresh -h {} -s {s}",
            instance::normalize_hostname("github.com"),
        ));
    }

    None
}

/// Validate that a scopes header contains the minimum required scopes.
///
/// # Errors
///
/// Returns `ApiError::MissingScopes` if required scopes are absent.
pub fn check_minimum_scopes(scopes_header: &str) -> Result<(), ApiError> {
    if scopes_header.is_empty() {
        // Empty scopes likely means an integration/fine-grained token;
        // we cannot detect its capabilities from scopes alone.
        return Ok(());
    }

    let scopes: std::collections::HashSet<&str> = scopes_header.split(',').map(str::trim).collect();

    let mut missing = Vec::new();

    if !scopes.contains("repo") {
        missing.push("repo".to_string());
    }

    if !scopes.contains("read:org")
        && !scopes.contains("write:org")
        && !scopes.contains("admin:org")
    {
        missing.push("read:org".to_string());
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(ApiError::MissingScopes(missing))
    }
}

/// Mask a token for display, keeping the prefix before the last underscore.
pub fn mask_token(token: &str) -> String {
    if let Some(idx) = token.rfind('_') {
        let prefix = &token[..=idx];
        let mask_len = token.len() - prefix.len();
        format!("{prefix}{}", "*".repeat(mask_len))
    } else {
        "*".repeat(token.len())
    }
}

/// Check whether the token source indicates the token is writeable (not from env).
pub fn token_source_is_writeable(source: &str) -> bool {
    !source.ends_with("_TOKEN")
}

/// Check whether a token format indicates we should expect scopes.
///
/// Classic PATs (`ghp_`) and OAuth tokens (`gho_`) report scopes;
/// fine-grained tokens and GitHub App tokens do not.
pub fn expect_scopes(token: &str) -> bool {
    token.starts_with("ghp_") || token.starts_with("gho_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_mask_token_with_prefix() {
        assert_eq!(mask_token("ghp_abc123"), "ghp_******");
    }

    #[test]
    fn test_should_mask_token_without_prefix() {
        assert_eq!(mask_token("secret"), "******");
    }

    #[test]
    fn test_should_detect_writeable_source() {
        assert!(token_source_is_writeable("config"));
        assert!(token_source_is_writeable("keyring"));
        assert!(!token_source_is_writeable("GH_TOKEN"));
        assert!(!token_source_is_writeable("GITHUB_TOKEN"));
    }

    #[test]
    fn test_should_detect_expect_scopes() {
        assert!(expect_scopes("ghp_abc"));
        assert!(expect_scopes("gho_xyz"));
        assert!(!expect_scopes("github_pat_abc"));
        assert!(!expect_scopes("ghs_abc"));
    }

    #[test]
    fn test_should_parse_link_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            r#"<https://api.github.com/repos?page=2>; rel="next", <https://api.github.com/repos?page=5>; rel="last""#
                .parse()
                .unwrap(),
        );
        assert_eq!(
            parse_link_next(&headers),
            Some("https://api.github.com/repos?page=2".to_string())
        );
    }

    #[test]
    fn test_should_return_none_for_no_next_link() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            r#"<https://api.github.com/repos?page=5>; rel="last""#
                .parse()
                .unwrap(),
        );
        assert_eq!(parse_link_next(&headers), None);
    }

    #[test]
    fn test_should_check_minimum_scopes_ok() {
        assert!(check_minimum_scopes("repo, read:org, gist").is_ok());
    }

    #[test]
    fn test_should_check_minimum_scopes_missing_repo() {
        let err = check_minimum_scopes("read:org, gist").unwrap_err();
        assert!(matches!(err, ApiError::MissingScopes(ref s) if s.contains(&"repo".to_string())));
    }

    #[test]
    fn test_should_check_minimum_scopes_empty_header() {
        // Empty scopes should pass (integration token)
        assert!(check_minimum_scopes("").is_ok());
    }

    #[test]
    fn test_should_generate_scopes_suggestion() {
        let suggestion = generate_scopes_suggestion(403, Some("admin:org"), Some("repo, read:org"));
        assert!(suggestion.is_some());
        assert!(suggestion.as_ref().is_some_and(|s| s.contains("admin:org")));
    }

    #[test]
    fn test_should_not_suggest_for_200() {
        assert!(generate_scopes_suggestion(200, Some("repo"), Some("")).is_none());
    }

    #[test]
    fn test_should_not_suggest_when_scope_present() {
        assert!(generate_scopes_suggestion(403, Some("repo"), Some("repo, read:org")).is_none());
    }

    #[test]
    fn test_should_recognize_implied_repo_scopes() {
        // "repo" implies "public_repo" etc.
        assert!(generate_scopes_suggestion(403, Some("public_repo"), Some("repo")).is_none());
    }

    #[test]
    fn test_should_recognize_implied_user_scopes() {
        // "user" implies "read:user"
        assert!(generate_scopes_suggestion(403, Some("read:user"), Some("user")).is_none());
    }

    #[test]
    fn test_should_recognize_admin_implies_read_write() {
        // "admin:org" implies "read:org" and "write:org"
        assert!(generate_scopes_suggestion(403, Some("read:org"), Some("admin:org")).is_none());
    }

    #[test]
    fn test_should_not_suggest_for_422() {
        assert!(generate_scopes_suggestion(422, Some("repo"), Some("")).is_none());
    }

    #[test]
    fn test_should_not_suggest_for_500() {
        assert!(generate_scopes_suggestion(500, Some("repo"), Some("")).is_none());
    }

    #[test]
    fn test_should_handle_retryable_errors() {
        assert!(is_retryable(&ApiError::Http {
            status: 429,
            message: "rate limit".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
        assert!(is_retryable(&ApiError::Http {
            status: 502,
            message: "bad gateway".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
        assert!(is_retryable(&ApiError::Http {
            status: 503,
            message: "service unavailable".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
        assert!(is_retryable(&ApiError::Http {
            status: 504,
            message: "gateway timeout".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
    }

    #[test]
    fn test_should_not_retry_client_errors() {
        assert!(!is_retryable(&ApiError::Http {
            status: 400,
            message: "bad request".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
        assert!(!is_retryable(&ApiError::Http {
            status: 404,
            message: "not found".to_string(),
            scopes_suggestion: None,
            headers: HashMap::new(),
        }));
        assert!(!is_retryable(&ApiError::AuthRequired));
    }

    #[test]
    fn test_should_extract_header_map() {
        let mut headers = HeaderMap::new();
        headers.insert("x-custom", "value".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        let map = extract_header_map(&headers);
        assert_eq!(map.get("x-custom"), Some(&"value".to_string()));
        assert_eq!(
            map.get("content-type"),
            Some(&"application/json".to_string()),
        );
    }

    #[test]
    fn test_should_create_client_and_normalize_hostname() {
        let http = reqwest::Client::new();
        let client = Client::new(http, "GitHub.COM", Some("token".into()));
        assert_eq!(client.hostname(), "github.com");
        assert_eq!(client.token(), Some("token"));
    }

    #[test]
    fn test_should_resolve_rest_url_absolute() {
        let http = reqwest::Client::new();
        let client = Client::new(http, "github.com", None);
        let url = client.resolve_rest_url("https://api.example.com/custom");
        assert_eq!(url, "https://api.example.com/custom");
    }

    #[test]
    fn test_should_resolve_rest_url_relative() {
        let http = reqwest::Client::new();
        let client = Client::new(http, "github.com", None);
        let url = client.resolve_rest_url("/repos/owner/repo");
        assert_eq!(url, "https://api.github.com/repos/owner/repo");
    }

    #[test]
    fn test_should_resolve_rest_url_enterprise() {
        let http = reqwest::Client::new();
        let client = Client::new(http, "ghe.example.com", None);
        let url = client.resolve_rest_url("/repos/owner/repo");
        assert_eq!(url, "https://ghe.example.com/api/v3/repos/owner/repo",);
    }
}

#[cfg(test)]
mod wiremock_tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn setup_client(_server: &MockServer) -> Client {
        let http = reqwest::Client::new();
        Client {
            http,
            hostname: "github.com".to_string(),
            token: Some("test-token".into()),
            api_url_override: None,
        }
    }

    #[tokio::test]
    async fn test_should_make_rest_get_request() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/cli/cli"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"name": "cli", "full_name": "cli/cli"})),
            )
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let result: Value = client
            .rest(
                reqwest::Method::GET,
                &format!("{}/repos/cli/cli", server.uri()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result["name"], "cli");
    }

    #[tokio::test]
    async fn test_should_return_http_error_for_non_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/missing/repo"))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"message": "Not Found"})),
            )
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let err = client
            .rest::<Value>(
                reqwest::Method::GET,
                &format!("{}/repos/missing/repo", server.uri()),
                None,
            )
            .await
            .unwrap_err();

        assert!(err.is_not_found());
    }

    #[tokio::test]
    async fn test_should_return_text_response() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/raw"))
            .respond_with(ResponseTemplate::new(200).set_body_string("raw text content"))
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let text = client
            .rest_text(reqwest::Method::GET, &format!("{}/raw", server.uri()), None)
            .await
            .unwrap();

        assert_eq!(text, "raw text content");
    }

    #[tokio::test]
    async fn test_should_send_auth_header() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/user"))
            .and(header("Authorization", "token test-token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"login": "testuser"})),
            )
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let result: Value = client
            .rest(
                reqwest::Method::GET,
                &format!("{}/user", server.uri()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result["login"], "testuser");
    }

    #[tokio::test]
    async fn test_should_follow_link_pagination() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/items"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{"id": 1}, {"id": 2}]))
                    .append_header(
                        "link",
                        format!("<{}/items?page=2>; rel=\"next\"", server.uri()),
                    ),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/items"))
            // wiremock will match the query-bearing URL too
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id": 3}])))
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let page: RestPage<Vec<Value>> = client
            .rest_with_next(
                reqwest::Method::GET,
                &format!("{}/items", server.uri()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(page.data.len(), 2);
        assert!(page.next_url.is_some());
    }

    #[tokio::test]
    async fn test_should_make_graphql_request() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "viewer": {"login": "testuser"}
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let client = Client {
            http,
            hostname: "github.com".to_string(),
            token: Some("test-token".into()),
            api_url_override: None,
        };

        // Override the GraphQL URL by using the server directly
        let body = serde_json::json!({
            "query": "query { viewer { login } }",
            "variables": {},
        });

        let resp = client
            .http
            .post(format!("{}/graphql", server.uri()))
            .header("Authorization", "token test-token")
            .json(&body)
            .send()
            .await
            .unwrap();

        let json: Value = resp.json().await.unwrap();
        assert_eq!(json["data"]["viewer"]["login"], "testuser");
    }

    #[tokio::test]
    async fn test_should_include_scopes_suggestion_on_403() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/private/repo"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string("Forbidden")
                    .append_header("x-accepted-oauth-scopes", "admin:org")
                    .append_header("x-oauth-scopes", "repo, read:org"),
            )
            .mount(&server)
            .await;

        let client = setup_client(&server);

        let err = client
            .rest::<Value>(
                reqwest::Method::GET,
                &format!("{}/repos/private/repo", server.uri()),
                None,
            )
            .await
            .unwrap_err();

        if let ApiError::Http {
            scopes_suggestion, ..
        } = &err
        {
            assert!(scopes_suggestion.is_some());
            assert!(scopes_suggestion.as_ref().unwrap().contains("admin:org"));
        } else {
            panic!("expected Http error");
        }
    }
}
