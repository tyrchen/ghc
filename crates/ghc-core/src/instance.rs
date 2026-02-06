//! GitHub instance handling for different deployment types.
//!
//! Supports github.com, GitHub Enterprise Server, and GHE.com tenants.

use url::Url;

/// Known GitHub cloud hostname.
pub const GITHUB_COM: &str = "github.com";

/// GitHub localhost for development.
const LOCALHOST: &str = "github.localhost";

/// GHE.com tenant suffix.
const GHE_COM_SUFFIX: &str = ".ghe.com";

/// Normalize a GitHub hostname by removing protocol and trailing slashes.
pub fn normalize_hostname(host: &str) -> String {
    // Strip protocol if present
    let host = host
        .strip_prefix("https://")
        .or_else(|| host.strip_prefix("http://"))
        .unwrap_or(host);

    // Strip trailing slashes
    let host = host.trim_end_matches('/');

    // Normalize to lowercase
    host.to_lowercase()
}

/// Check if a hostname is a GitHub.com cloud instance.
pub fn is_github_com(host: &str) -> bool {
    let normalized = normalize_hostname(host);
    normalized == GITHUB_COM || normalized == LOCALHOST
}

/// Check if a hostname is a GHE.com tenant.
pub fn is_ghe_com(host: &str) -> bool {
    let normalized = normalize_hostname(host);
    normalized.ends_with(GHE_COM_SUFFIX)
}

/// Check if a hostname is an enterprise instance (not cloud, not GHE.com).
pub fn is_enterprise(host: &str) -> bool {
    !is_github_com(host) && !is_ghe_com(host)
}

/// Get the REST API base URL for a given hostname.
pub fn rest_url(host: &str) -> String {
    let normalized = normalize_hostname(host);
    if is_github_com(&normalized) {
        "https://api.github.com/".to_string()
    } else {
        format!("https://{normalized}/api/v3/")
    }
}

/// Get the GraphQL API endpoint for a given hostname.
pub fn graphql_url(host: &str) -> String {
    let normalized = normalize_hostname(host);
    if is_github_com(&normalized) {
        "https://api.github.com/graphql".to_string()
    } else {
        format!("https://{normalized}/api/graphql")
    }
}

/// Get the Gist hostname for a given GitHub hostname.
pub fn gist_host(host: &str) -> String {
    let normalized = normalize_hostname(host);
    if is_github_com(&normalized) {
        "gist.github.com".to_string()
    } else {
        normalized
    }
}

/// Get the HTTPS URL prefix for a hostname (e.g., `https://github.com/`).
pub fn host_prefix(host: &str) -> String {
    format!("https://{}/", normalize_hostname(host))
}

/// Get the hostname from a URL, normalizing it.
pub fn host_from_url(u: &Url) -> Option<String> {
    u.host_str().map(normalize_hostname)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("GitHub.com", "github.com")]
    #[case("GITHUB.COM", "github.com")]
    #[case("https://github.com/", "github.com")]
    #[case("http://github.com/", "github.com")]
    #[case("https://my-ghe.example.com", "my-ghe.example.com")]
    #[case("https://ghe.io///", "ghe.io")]
    #[case("github.com", "github.com")]
    #[case("github.com/", "github.com")]
    #[case("TENANT.GHE.COM", "tenant.ghe.com")]
    fn test_should_normalize_hostname(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalize_hostname(input), expected);
    }

    #[test]
    fn test_should_normalize_already_clean_hostname() {
        assert_eq!(normalize_hostname("github.com"), "github.com");
    }

    #[rstest]
    #[case("github.com", true)]
    #[case("GitHub.com", true)]
    #[case("GITHUB.COM", true)]
    #[case("https://github.com", true)]
    #[case("github.localhost", true)]
    #[case("enterprise.example.com", false)]
    #[case("tenant.ghe.com", false)]
    #[case("github.com.evil.com", false)]
    fn test_should_detect_github_com(#[case] host: &str, #[case] expected: bool) {
        assert_eq!(is_github_com(host), expected);
    }

    #[rstest]
    #[case("tenant.ghe.com", true)]
    #[case("my-org.ghe.com", true)]
    #[case("TENANT.GHE.COM", true)]
    #[case("https://tenant.ghe.com/", true)]
    #[case("github.com", false)]
    #[case("enterprise.example.com", false)]
    #[case("ghe.com", false)]
    fn test_should_detect_ghe_com(#[case] host: &str, #[case] expected: bool) {
        assert_eq!(is_ghe_com(host), expected);
    }

    #[rstest]
    #[case("enterprise.example.com", true)]
    #[case("git.mycompany.net", true)]
    #[case("github.com", false)]
    #[case("github.localhost", false)]
    #[case("tenant.ghe.com", false)]
    fn test_should_detect_enterprise(#[case] host: &str, #[case] expected: bool) {
        assert_eq!(is_enterprise(host), expected);
    }

    #[rstest]
    #[case("github.com", "https://api.github.com/")]
    #[case("GitHub.com", "https://api.github.com/")]
    #[case("github.localhost", "https://api.github.com/")]
    #[case("ghe.example.com", "https://ghe.example.com/api/v3/")]
    #[case("tenant.ghe.com", "https://tenant.ghe.com/api/v3/")]
    fn test_should_generate_rest_urls(#[case] host: &str, #[case] expected: &str) {
        assert_eq!(rest_url(host), expected);
    }

    #[rstest]
    #[case("github.com", "https://api.github.com/graphql")]
    #[case("GitHub.com", "https://api.github.com/graphql")]
    #[case("github.localhost", "https://api.github.com/graphql")]
    #[case("ghe.example.com", "https://ghe.example.com/api/graphql")]
    #[case("tenant.ghe.com", "https://tenant.ghe.com/api/graphql")]
    fn test_should_generate_graphql_urls(#[case] host: &str, #[case] expected: &str) {
        assert_eq!(graphql_url(host), expected);
    }

    #[rstest]
    #[case("github.com", "gist.github.com")]
    #[case("github.localhost", "gist.github.com")]
    #[case("ghe.example.com", "ghe.example.com")]
    fn test_should_generate_gist_host(#[case] host: &str, #[case] expected: &str) {
        assert_eq!(gist_host(host), expected);
    }

    #[test]
    fn test_should_extract_host_from_url() {
        let u = Url::parse("https://github.com/cli/cli").unwrap();
        assert_eq!(host_from_url(&u), Some("github.com".to_string()));

        let u = Url::parse("https://GHE.IO/org/repo").unwrap();
        assert_eq!(host_from_url(&u), Some("ghe.io".to_string()));
    }

    #[test]
    fn test_should_return_none_for_url_without_host() {
        let u = Url::parse("file:///tmp/repo").unwrap();
        assert!(host_from_url(&u).is_none());
    }

    // --- property-based tests ---

    mod prop {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn normalize_hostname_is_idempotent(host in "[a-z0-9]{1,20}(\\.[a-z]{2,6}){1,3}") {
                let once = normalize_hostname(&host);
                let twice = normalize_hostname(&once);
                prop_assert_eq!(&once, &twice);
            }

            #[test]
            fn normalize_hostname_always_lowercase(host in "[A-Za-z0-9.]{1,40}") {
                let result = normalize_hostname(&host);
                let lowered = result.to_lowercase();
                prop_assert_eq!(result, lowered);
            }

            #[test]
            fn normalize_hostname_strips_trailing_slashes(
                host in "[a-z]{1,10}\\.[a-z]{2,5}",
                slashes in "/{0,5}",
            ) {
                let input = format!("{host}{slashes}");
                let result = normalize_hostname(&input);
                prop_assert!(!result.ends_with('/'));
            }

            #[test]
            fn is_github_com_consistent_with_normalize(
                host in "(github\\.com|GITHUB\\.COM|GitHub\\.Com)",
            ) {
                prop_assert!(is_github_com(&host));
            }
        }
    }
}
