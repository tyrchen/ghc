//! Repository representation and parsing.
//!
//! Maps from Go's `internal/ghrepo` package.

use std::fmt;

use url::Url;

use crate::instance::{self, GITHUB_COM};

/// A GitHub repository identified by owner, name, and host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Repo {
    owner: String,
    name: String,
    host: String,
}

impl Repo {
    /// Create a new repo on github.com.
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
            host: GITHUB_COM.to_string(),
        }
    }

    /// Create a new repo with a specific host.
    pub fn with_host(
        owner: impl Into<String>,
        name: impl Into<String>,
        host: impl Into<String>,
    ) -> Self {
        Self {
            owner: owner.into(),
            name: name.into(),
            host: instance::normalize_hostname(&host.into()),
        }
    }

    /// Parse a "OWNER/REPO" or "HOST/OWNER/REPO" string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as a repository reference.
    pub fn from_full_name(nwo: &str) -> Result<Self, RepoParseError> {
        let parts: Vec<&str> = nwo.split('/').collect();
        match parts.len() {
            2 => {
                if parts[0].is_empty() || parts[1].is_empty() {
                    return Err(RepoParseError::InvalidFormat(nwo.to_string()));
                }
                Ok(Self::new(parts[0], parts[1]))
            }
            3.. => {
                if parts[0].is_empty() || parts[1].is_empty() || parts[2].is_empty() {
                    return Err(RepoParseError::InvalidFormat(nwo.to_string()));
                }
                Ok(Self::with_host(parts[1], parts[2], parts[0]))
            }
            _ => Err(RepoParseError::InvalidFormat(nwo.to_string())),
        }
    }

    /// Parse a repository from a git remote URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL cannot be parsed as a repository reference.
    pub fn from_url(u: &Url) -> Result<Self, RepoParseError> {
        let host = u
            .host_str()
            .ok_or_else(|| RepoParseError::InvalidUrl(u.to_string()))?;

        let path = u.path().trim_start_matches('/').trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();

        if parts.len() < 2 {
            return Err(RepoParseError::InvalidUrl(u.to_string()));
        }

        Ok(Self::with_host(parts[0], parts[1], host))
    }

    /// Repository owner (user or organization).
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Repository name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// GitHub hostname.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Full name as "OWNER/REPO".
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

impl fmt::Display for Repo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if instance::is_github_com(&self.host) {
            write!(f, "{}/{}", self.owner, self.name)
        } else {
            write!(f, "{}/{}/{}", self.host, self.owner, self.name)
        }
    }
}

/// Errors from parsing repository references.
#[derive(Debug, thiserror::Error)]
pub enum RepoParseError {
    /// String does not match expected format.
    #[error("expected OWNER/REPO or HOST/OWNER/REPO format, got {0:?}")]
    InvalidFormat(String),
    /// URL does not contain repository information.
    #[error("cannot extract repository from URL: {0}")]
    InvalidUrl(String),
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("cli/cli", "cli", "cli", "github.com")]
    #[case("owner/repo-name", "owner", "repo-name", "github.com")]
    #[case("my-org/my.repo", "my-org", "my.repo", "github.com")]
    fn test_should_parse_owner_repo(
        #[case] input: &str,
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
    ) {
        let repo = Repo::from_full_name(input).unwrap();
        assert_eq!(repo.owner(), owner);
        assert_eq!(repo.name(), name);
        assert_eq!(repo.host(), host);
    }

    #[rstest]
    #[case("ghe.io/my-org/my-repo", "my-org", "my-repo", "ghe.io")]
    #[case("enterprise.com/team/project", "team", "project", "enterprise.com")]
    fn test_should_parse_host_owner_repo(
        #[case] input: &str,
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
    ) {
        let repo = Repo::from_full_name(input).unwrap();
        assert_eq!(repo.owner(), owner);
        assert_eq!(repo.name(), name);
        assert_eq!(repo.host(), host);
    }

    #[rstest]
    #[case("just-a-name")]
    #[case("/repo")]
    #[case("owner/")]
    #[case("")]
    #[case("//")]
    #[case("/")]
    #[case("host//repo")]
    fn test_should_reject_invalid_format(#[case] input: &str) {
        assert!(Repo::from_full_name(input).is_err());
    }

    #[test]
    fn test_should_return_invalid_format_error_message() {
        let err = Repo::from_full_name("bad").unwrap_err();
        assert!(err.to_string().contains("bad"));
    }

    #[rstest]
    #[case("https://github.com/cli/cli.git", "cli", "cli", "github.com")]
    #[case("https://github.com/cli/cli", "cli", "cli", "github.com")]
    #[case("https://ghe.io/org/repo.git", "org", "repo", "ghe.io")]
    #[case(
        "https://github.com/owner/repo/extra/path",
        "owner",
        "repo",
        "github.com"
    )]
    fn test_should_parse_url(
        #[case] url_str: &str,
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
    ) {
        let u = Url::parse(url_str).unwrap();
        let repo = Repo::from_url(&u).unwrap();
        assert_eq!(repo.owner(), owner);
        assert_eq!(repo.name(), name);
        assert_eq!(repo.host(), host);
    }

    #[test]
    fn test_should_reject_url_without_enough_path_segments() {
        let u = Url::parse("https://github.com/only-owner").unwrap();
        assert!(Repo::from_url(&u).is_err());
    }

    #[test]
    fn test_should_reject_url_without_host() {
        let u = Url::parse("file:///some/path").unwrap();
        assert!(Repo::from_url(&u).is_err());
    }

    #[test]
    fn test_should_display_github_com_repo_as_owner_name() {
        let repo = Repo::new("cli", "cli");
        assert_eq!(repo.to_string(), "cli/cli");
    }

    #[test]
    fn test_should_display_enterprise_repo_with_host() {
        let repo = Repo::with_host("org", "repo", "ghe.io");
        assert_eq!(repo.to_string(), "ghe.io/org/repo");
    }

    #[test]
    fn test_should_return_full_name() {
        let repo = Repo::new("cli", "cli");
        assert_eq!(repo.full_name(), "cli/cli");

        let repo = Repo::with_host("org", "repo", "ghe.io");
        assert_eq!(repo.full_name(), "org/repo");
    }

    #[test]
    fn test_should_normalize_host_in_with_host() {
        let repo = Repo::with_host("org", "repo", "https://GHE.IO/");
        assert_eq!(repo.host(), "ghe.io");
    }

    #[test]
    fn test_should_be_equal_when_same_fields() {
        let a = Repo::new("cli", "cli");
        let b = Repo::new("cli", "cli");
        assert_eq!(a, b);
    }

    #[test]
    fn test_should_not_be_equal_when_different_host() {
        let a = Repo::new("cli", "cli");
        let b = Repo::with_host("cli", "cli", "ghe.io");
        assert_ne!(a, b);
    }

    #[test]
    fn test_should_be_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Repo::new("cli", "cli"));
        set.insert(Repo::new("cli", "cli"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_should_be_cloneable() {
        let repo = Repo::new("cli", "cli");
        let clone = repo.clone();
        assert_eq!(repo, clone);
    }

    // --- property-based tests ---

    mod prop {
        use proptest::prelude::*;

        use super::super::*;

        proptest! {
            #[test]
            fn roundtrip_parse_display_for_github_com(
                owner in "[a-zA-Z][a-zA-Z0-9-]{0,15}",
                name in "[a-zA-Z][a-zA-Z0-9._-]{0,15}",
            ) {
                let input = format!("{owner}/{name}");
                let repo = Repo::from_full_name(&input)?;
                prop_assert_eq!(repo.owner(), owner.as_str());
                prop_assert_eq!(repo.name(), name.as_str());
                // Display should roundtrip for github.com repos
                let displayed = repo.to_string();
                prop_assert_eq!(displayed, format!("{owner}/{name}"));
            }

            #[test]
            fn from_full_name_always_returns_github_com_host(
                owner in "[a-zA-Z][a-zA-Z0-9]{0,10}",
                name in "[a-zA-Z][a-zA-Z0-9]{0,10}",
            ) {
                let input = format!("{owner}/{name}");
                let repo = Repo::from_full_name(&input)?;
                prop_assert_eq!(repo.host(), "github.com");
            }

            #[test]
            fn full_name_contains_owner_and_name(
                owner in "[a-zA-Z][a-zA-Z0-9]{0,10}",
                name in "[a-zA-Z][a-zA-Z0-9]{0,10}",
            ) {
                let repo = Repo::new(&owner, &name);
                let full = repo.full_name();
                prop_assert!(full.contains(&owner));
                prop_assert!(full.contains(&name));
            }
        }
    }
}
