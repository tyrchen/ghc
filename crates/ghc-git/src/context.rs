//! Repository context resolution.
//!
//! Maps from Go's `context/` package. Determines the base repository
//! from local git remotes and their `gh-resolved` configuration.

use ghc_core::repo::Repo;

use crate::remote::Remote;

/// Resolve the base repository from a list of remotes.
///
/// Resolution priority:
/// 1. Remote with `gh-resolved` set to `"base"`
/// 2. Remote named "upstream" with a resolved repo
/// 3. Remote named "github" with a resolved repo
/// 4. Remote named "origin" with a resolved repo
/// 5. First remote with a resolved repo
///
/// The `gh-resolved` key is set by `ghc repo set-default` and takes
/// highest priority. This allows users to explicitly choose their
/// base repo in fork scenarios.
pub fn resolve_base_repo(remotes: &[Remote]) -> Option<&Repo> {
    // Check for explicit gh-resolved = "base" first
    for remote in remotes {
        if remote.resolved == "base"
            && let Some(ref repo) = remote.repo
        {
            return Some(repo);
        }
    }

    // Remotes are already sorted by priority in Remote::parse_remotes
    // (upstream > github > origin > others)
    remotes.iter().find_map(|r| r.repo.as_ref())
}

/// Filter remotes to only those pointing to a specific host.
pub fn filter_remotes_by_host<'a>(remotes: &'a [Remote], host: &str) -> Vec<&'a Remote> {
    let normalized = ghc_core::instance::normalize_hostname(host);
    remotes
        .iter()
        .filter(|r| {
            r.repo.as_ref().is_some_and(|repo| {
                ghc_core::instance::normalize_hostname(repo.host()) == normalized
            })
        })
        .collect()
}

/// Find a remote by its name.
pub fn find_remote_by_name<'a>(remotes: &'a [Remote], name: &str) -> Option<&'a Remote> {
    remotes.iter().find(|r| r.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_remote(name: &str, owner: &str, repo_name: &str, resolved: &str) -> Remote {
        Remote {
            name: name.to_string(),
            fetch_url: format!("https://github.com/{owner}/{repo_name}.git"),
            push_url: None,
            repo: Some(Repo::new(owner, repo_name)),
            resolved: resolved.to_string(),
        }
    }

    #[test]
    fn test_should_resolve_by_gh_resolved_base() {
        let remotes = vec![
            make_remote("origin", "user", "fork", ""),
            make_remote("upstream", "org", "repo", "base"),
        ];
        let repo = resolve_base_repo(&remotes).unwrap();
        assert_eq!(repo.owner(), "org");
        assert_eq!(repo.name(), "repo");
    }

    #[test]
    fn test_should_resolve_by_sort_priority_when_no_resolved() {
        let remotes = vec![
            make_remote("upstream", "org", "repo", ""),
            make_remote("origin", "user", "fork", ""),
        ];
        let repo = resolve_base_repo(&remotes).unwrap();
        // Upstream comes first in sorted order
        assert_eq!(repo.owner(), "org");
    }

    #[test]
    fn test_should_return_none_for_empty_remotes() {
        let remotes: Vec<Remote> = vec![];
        assert!(resolve_base_repo(&remotes).is_none());
    }

    #[test]
    fn test_should_filter_by_host() {
        let remotes = vec![
            make_remote("origin", "user", "repo", ""),
            Remote {
                name: "ghe".to_string(),
                fetch_url: "https://ghe.example.com/org/repo.git".to_string(),
                push_url: None,
                repo: Some(Repo::with_host("org", "repo", "ghe.example.com")),
                resolved: String::new(),
            },
        ];

        let filtered = filter_remotes_by_host(&remotes, "github.com");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "origin");

        let filtered = filter_remotes_by_host(&remotes, "ghe.example.com");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "ghe");
    }

    #[test]
    fn test_should_find_remote_by_name() {
        let remotes = vec![
            make_remote("origin", "user", "repo", ""),
            make_remote("upstream", "org", "repo", ""),
        ];
        let r = find_remote_by_name(&remotes, "upstream").unwrap();
        assert_eq!(r.name, "upstream");

        assert!(find_remote_by_name(&remotes, "nonexistent").is_none());
    }

    #[test]
    fn test_should_prefer_gh_resolved_over_sort_priority() {
        // origin has resolved=base, upstream does not
        let remotes = vec![
            make_remote("upstream", "org", "upstream-repo", ""),
            make_remote("origin", "user", "fork-repo", "base"),
        ];
        let repo = resolve_base_repo(&remotes).unwrap();
        // Should pick origin because it has resolved=base
        assert_eq!(repo.owner(), "user");
        assert_eq!(repo.name(), "fork-repo");
    }

    #[test]
    fn test_should_skip_remotes_without_repo() {
        let remotes = vec![
            Remote {
                name: "broken".to_string(),
                fetch_url: "not-a-valid-url".to_string(),
                push_url: None,
                repo: None,
                resolved: String::new(),
            },
            make_remote("origin", "user", "repo", ""),
        ];
        let repo = resolve_base_repo(&remotes).unwrap();
        assert_eq!(repo.owner(), "user");
    }

    #[test]
    fn test_should_return_empty_for_host_filter_no_match() {
        let remotes = vec![make_remote("origin", "user", "repo", "")];
        let filtered = filter_remotes_by_host(&remotes, "ghe.example.com");
        assert!(filtered.is_empty());
    }
}
