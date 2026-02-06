//! Git URL parsing utilities.

use url::Url;

use ghc_core::repo::Repo;

/// Translate a GitHub repository reference to a clone URL.
pub fn clone_url(repo: &Repo, protocol: &str) -> String {
    match protocol {
        "ssh" => format!("git@{}:{}/{}.git", repo.host(), repo.owner(), repo.name()),
        _ => format!(
            "https://{}/{}/{}.git",
            repo.host(),
            repo.owner(),
            repo.name()
        ),
    }
}

/// Check if a string looks like a git URL.
pub fn is_url(u: &str) -> bool {
    u.starts_with("git@") || is_supported_protocol(u)
}

/// Parse a git remote URL and normalize it.
///
/// Handles scp-like SSH syntax (`git@host:owner/repo`) and standard URLs.
///
/// # Errors
///
/// Returns an error if the URL cannot be parsed.
pub fn parse_url(raw_url: &str) -> Result<Url, url::ParseError> {
    // Normalize scheme aliases at the string level first, because the `url`
    // crate refuses `set_scheme` transitions between special and non-special
    // schemes (e.g., `git+https` -> `https`).
    let pre_normalized = if raw_url.starts_with("git+https:") {
        raw_url.replacen("git+https:", "https:", 1)
    } else if raw_url.starts_with("git+ssh:") {
        raw_url.replacen("git+ssh:", "ssh:", 1)
    } else {
        raw_url.to_string()
    };

    let normalized = if !is_possible_protocol(&pre_normalized)
        && pre_normalized.contains(':')
        && !pre_normalized.contains('\\')
    {
        // Support scp-like syntax for SSH protocol
        format!("ssh://{}", pre_normalized.replacen(':', "/", 1))
    } else {
        pre_normalized
    };

    let mut u = Url::parse(&normalized)?;

    if u.scheme() == "ssh" {
        // Remove leading double slashes from path
        let path = u.path().to_string();
        if path.starts_with("//") {
            u.set_path(path.trim_start_matches('/'));
        }

        // Remove port from host if present
        let host = u.host_str().unwrap_or("").to_string();
        if let Some(stripped) = host.strip_suffix(&format!(":{}", u.port().unwrap_or(0))) {
            u.set_host(Some(stripped)).ok();
        }
    }

    Ok(u)
}

fn is_supported_protocol(u: &str) -> bool {
    u.starts_with("ssh:")
        || u.starts_with("git+ssh:")
        || u.starts_with("git:")
        || u.starts_with("http:")
        || u.starts_with("git+https:")
        || u.starts_with("https:")
}

fn is_possible_protocol(u: &str) -> bool {
    is_supported_protocol(u)
        || u.starts_with("ftp:")
        || u.starts_with("ftps:")
        || u.starts_with("file:")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // --- clone_url tests ---

    #[rstest]
    #[case("cli", "cli", "github.com", "https", "https://github.com/cli/cli.git")]
    #[case("org", "repo", "ghe.io", "https", "https://ghe.io/org/repo.git")]
    #[case("cli", "cli", "github.com", "ssh", "git@github.com:cli/cli.git")]
    #[case("org", "repo", "ghe.io", "ssh", "git@ghe.io:org/repo.git")]
    fn test_should_generate_clone_url(
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
        #[case] protocol: &str,
        #[case] expected: &str,
    ) {
        let repo = Repo::with_host(owner, name, host);
        assert_eq!(clone_url(&repo, protocol), expected);
    }

    #[test]
    fn test_should_default_to_https_for_unknown_protocol() {
        let repo = Repo::new("cli", "cli");
        let url = clone_url(&repo, "unknown");
        assert!(url.starts_with("https://"));
    }

    // --- is_url tests ---

    #[rstest]
    #[case("git@github.com:cli/cli.git", true)]
    #[case("https://github.com/cli/cli.git", true)]
    #[case("ssh://git@github.com/cli/cli.git", true)]
    #[case("git+https://github.com/cli/cli.git", true)]
    #[case("git+ssh://git@github.com/cli/cli.git", true)]
    #[case("http://github.com/cli/cli", true)]
    #[case("git://github.com/cli/cli", true)]
    #[case("cli/cli", false)]
    #[case("just-a-name", false)]
    #[case("", false)]
    fn test_should_detect_url(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_url(input), expected);
    }

    // --- parse_url tests ---

    #[test]
    fn test_should_parse_https_url() {
        let u = parse_url("https://github.com/cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host_str(), Some("github.com"));
        assert_eq!(u.path(), "/cli/cli.git");
    }

    #[test]
    fn test_should_parse_http_url() {
        let u = parse_url("http://github.com/cli/cli").unwrap();
        assert_eq!(u.scheme(), "http");
        assert_eq!(u.host_str(), Some("github.com"));
    }

    #[test]
    fn test_should_parse_ssh_scp_url() {
        let u = parse_url("git@github.com:cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "ssh");
        assert_eq!(u.host_str(), Some("github.com"));
    }

    #[test]
    fn test_should_parse_ssh_protocol_url() {
        let u = parse_url("ssh://git@github.com/cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "ssh");
        assert_eq!(u.host_str(), Some("github.com"));
    }

    #[test]
    fn test_should_normalize_git_plus_https() {
        let u = parse_url("git+https://github.com/cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "https");
    }

    #[test]
    fn test_should_normalize_git_plus_ssh() {
        let u = parse_url("git+ssh://git@github.com/cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "ssh");
    }

    #[test]
    fn test_should_parse_enterprise_ssh_url() {
        let u = parse_url("git@ghe.example.com:org/repo.git").unwrap();
        assert_eq!(u.scheme(), "ssh");
        assert_eq!(u.host_str(), Some("ghe.example.com"));
    }

    #[test]
    fn test_should_parse_git_protocol_url() {
        let u = parse_url("git://github.com/cli/cli.git").unwrap();
        assert_eq!(u.scheme(), "git");
        assert_eq!(u.host_str(), Some("github.com"));
    }

    // --- is_supported_protocol tests ---

    #[test]
    fn test_should_recognize_supported_protocols() {
        assert!(is_supported_protocol("ssh://host"));
        assert!(is_supported_protocol("git+ssh://host"));
        assert!(is_supported_protocol("git://host"));
        assert!(is_supported_protocol("http://host"));
        assert!(is_supported_protocol("git+https://host"));
        assert!(is_supported_protocol("https://host"));
        assert!(!is_supported_protocol("ftp://host"));
        assert!(!is_supported_protocol("file://host"));
    }

    // --- is_possible_protocol tests ---

    #[test]
    fn test_should_recognize_possible_protocols() {
        assert!(is_possible_protocol("https://host"));
        assert!(is_possible_protocol("ftp://host"));
        assert!(is_possible_protocol("ftps://host"));
        assert!(is_possible_protocol("file:///path"));
        assert!(!is_possible_protocol("owner/repo"));
    }

    // --- property-based tests ---

    mod prop {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn parse_https_url_always_has_host(
                host in "[a-z]{1,10}\\.[a-z]{2,5}",
                owner in "[a-z]{1,10}",
                repo in "[a-z]{1,10}",
            ) {
                let url_str = format!("https://{host}/{owner}/{repo}.git");
                let u = parse_url(&url_str)?;
                prop_assert!(u.host_str().is_some());
                prop_assert_eq!(u.scheme(), "https");
            }

            #[test]
            fn parse_ssh_scp_url_always_has_host(
                host in "[a-z]{1,10}\\.[a-z]{2,5}",
                owner in "[a-z]{1,10}",
                repo in "[a-z]{1,10}",
            ) {
                let url_str = format!("git@{host}:{owner}/{repo}.git");
                let u = parse_url(&url_str)?;
                prop_assert!(u.host_str().is_some());
                prop_assert_eq!(u.scheme(), "ssh");
            }

            #[test]
            fn clone_url_https_always_ends_with_git(
                owner in "[a-z]{1,10}",
                name in "[a-z]{1,10}",
            ) {
                let repo = ghc_core::repo::Repo::new(&owner, &name);
                let result = clone_url(&repo, "https");
                prop_assert!(std::path::Path::new(&result)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("git")));
                prop_assert!(result.starts_with("https://"));
            }

            #[test]
            fn clone_url_ssh_uses_git_at(
                owner in "[a-z]{1,10}",
                name in "[a-z]{1,10}",
            ) {
                let repo = ghc_core::repo::Repo::new(&owner, &name);
                let result = clone_url(&repo, "ssh");
                prop_assert!(result.starts_with("git@"));
                prop_assert!(std::path::Path::new(&result)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("git")));
            }

            #[test]
            fn is_url_true_for_any_protocol_prefix(
                proto in "(https|http|ssh|git)://",
                rest in "[a-z.]{1,20}",
            ) {
                let url_str = format!("{proto}{rest}");
                prop_assert!(is_url(&url_str));
            }
        }
    }
}
