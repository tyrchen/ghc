//! Git remote parsing and management.

use url::Url;

use ghc_core::repo::Repo;

/// A git remote with its name and URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// Remote name (e.g., "origin", "upstream").
    pub name: String,
    /// Fetch URL.
    pub fetch_url: String,
    /// Push URL (may differ from fetch).
    pub push_url: Option<String>,
    /// Resolved repository info.
    pub repo: Option<Repo>,
    /// The `gh-resolved` config value (e.g., "base", "other").
    pub resolved: String,
}

impl Remote {
    /// Parse `git remote -v` output into a list of remotes.
    pub fn parse_remotes(output: &str) -> Vec<Self> {
        let mut remotes: Vec<Self> = Vec::new();

        for line in output.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }

            let name = parts[0];
            let url_str = parts[1];
            let direction = parts[2].trim_matches(|c| c == '(' || c == ')');

            let repo = parse_remote_url(url_str);

            if let Some(existing) = remotes.iter_mut().find(|r| r.name == name) {
                if direction == "push" {
                    existing.push_url = Some(url_str.to_string());
                }
            } else {
                remotes.push(Self {
                    name: name.to_string(),
                    fetch_url: url_str.to_string(),
                    push_url: if direction == "push" {
                        Some(url_str.to_string())
                    } else {
                        None
                    },
                    repo,
                    resolved: String::new(),
                });
            }
        }

        // Sort by priority: upstream > github > origin > others
        remotes.sort_by_key(|a| remote_sort_key(&a.name));
        remotes
    }

    /// Populate `resolved` field from `git config --get-regexp` output.
    ///
    /// Lines should be in the format: `remote.<name>.gh-resolved <value>`
    pub fn populate_resolved(remotes: &mut [Self], config_output: &str) {
        for line in config_output.lines() {
            let Some((key_part, value)) = line.split_once(' ') else {
                continue;
            };
            // key_part is like "remote.origin.gh-resolved"
            let parts: Vec<&str> = key_part.splitn(3, '.').collect();
            if parts.len() < 2 {
                continue;
            }
            let remote_name = parts[1];
            for remote in remotes.iter_mut() {
                if remote.name == remote_name {
                    remote.resolved = value.to_string();
                    break;
                }
            }
        }
    }
}

fn remote_sort_key(name: &str) -> u8 {
    match name {
        "upstream" => 0,
        "github" => 1,
        "origin" => 2,
        _ => 3,
    }
}

/// Parse a git remote URL into a Repo.
pub fn parse_remote_url(url_str: &str) -> Option<Repo> {
    // Try SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url_str.strip_prefix("git@") {
        let colon_pos = rest.find(':')?;
        let host = &rest[..colon_pos];
        let path = rest[colon_pos + 1..].trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return Some(Repo::with_host(parts[0], parts[1], host));
        }
    }

    // Try SSH format: ssh://git@github.com/owner/repo.git
    if url_str.starts_with("ssh://")
        && let Ok(u) = Url::parse(url_str)
    {
        return Repo::from_url(&u).ok();
    }

    // Try HTTPS format
    if let Ok(u) = Url::parse(url_str) {
        return Repo::from_url(&u).ok();
    }

    None
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // --- parse_remotes tests ---

    #[test]
    fn test_should_parse_remotes_and_sort_by_priority() {
        let output = "\
origin\thttps://github.com/user/repo.git (fetch)
origin\thttps://github.com/user/repo.git (push)
upstream\thttps://github.com/org/repo.git (fetch)
upstream\thttps://github.com/org/repo.git (push)";

        let remotes = Remote::parse_remotes(output);
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "upstream");
        assert_eq!(remotes[1].name, "origin");
    }

    #[test]
    fn test_should_parse_github_remote_with_high_priority() {
        let output = "\
origin\thttps://github.com/user/repo.git (fetch)
github\thttps://github.com/org/repo.git (fetch)
fork\thttps://github.com/fork/repo.git (fetch)";

        let remotes = Remote::parse_remotes(output);
        assert_eq!(remotes.len(), 3);
        assert_eq!(remotes[0].name, "github");
        assert_eq!(remotes[1].name, "origin");
        assert_eq!(remotes[2].name, "fork");
    }

    #[test]
    fn test_should_merge_push_url_into_existing_remote() {
        let output = "\
origin\thttps://github.com/user/repo.git (fetch)
origin\tgit@github.com:user/repo.git (push)";

        let remotes = Remote::parse_remotes(output);
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].fetch_url, "https://github.com/user/repo.git",);
        assert_eq!(
            remotes[0].push_url,
            Some("git@github.com:user/repo.git".to_string()),
        );
    }

    #[test]
    fn test_should_resolve_repo_from_remote_url() {
        let output = "origin\thttps://github.com/cli/cli.git (fetch)";
        let remotes = Remote::parse_remotes(output);
        let repo = remotes[0].repo.as_ref().unwrap();
        assert_eq!(repo.owner(), "cli");
        assert_eq!(repo.name(), "cli");
    }

    #[test]
    fn test_should_handle_empty_input() {
        let remotes = Remote::parse_remotes("");
        assert!(remotes.is_empty());
    }

    #[test]
    fn test_should_skip_malformed_lines() {
        let output = "origin\n\nmalformed line\norigin\thttps://github.com/a/b (fetch)";
        let remotes = Remote::parse_remotes(output);
        assert_eq!(remotes.len(), 1);
    }

    // --- parse_remote_url tests ---

    #[rstest]
    #[case("git@github.com:cli/cli.git", "cli", "cli", "github.com")]
    #[case("git@github.com:owner/repo.git", "owner", "repo", "github.com")]
    #[case("git@ghe.io:org/project.git", "org", "project", "ghe.io")]
    fn test_should_parse_ssh_url(
        #[case] url: &str,
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
    ) {
        let repo = parse_remote_url(url).unwrap();
        assert_eq!(repo.owner(), owner);
        assert_eq!(repo.name(), name);
        assert_eq!(repo.host(), host);
    }

    #[rstest]
    #[case("https://github.com/cli/cli.git", "cli", "cli", "github.com")]
    #[case("https://github.com/cli/cli", "cli", "cli", "github.com")]
    #[case("https://ghe.io/org/repo.git", "org", "repo", "ghe.io")]
    fn test_should_parse_https_url(
        #[case] url: &str,
        #[case] owner: &str,
        #[case] name: &str,
        #[case] host: &str,
    ) {
        let repo = parse_remote_url(url).unwrap();
        assert_eq!(repo.owner(), owner);
        assert_eq!(repo.name(), name);
        assert_eq!(repo.host(), host);
    }

    #[test]
    fn test_should_parse_ssh_protocol_url() {
        let repo = parse_remote_url("ssh://git@github.com/cli/cli.git").unwrap();
        assert_eq!(repo.owner(), "cli");
        assert_eq!(repo.name(), "cli");
        assert_eq!(repo.host(), "github.com");
    }

    #[rstest]
    #[case("not-a-url")]
    #[case("")]
    #[case("ftp://example.com")]
    fn test_should_return_none_for_invalid_url(#[case] url: &str) {
        assert!(parse_remote_url(url).is_none());
    }

    // --- populate_resolved tests ---

    #[test]
    fn test_should_populate_resolved() {
        let output = "\
origin\thttps://github.com/user/repo.git (fetch)
upstream\thttps://github.com/org/repo.git (fetch)";

        let mut remotes = Remote::parse_remotes(output);
        let config = "remote.origin.gh-resolved base\nremote.upstream.gh-resolved other";
        Remote::populate_resolved(&mut remotes, config);

        let origin = remotes.iter().find(|r| r.name == "origin").unwrap();
        assert_eq!(origin.resolved, "base");

        let upstream = remotes.iter().find(|r| r.name == "upstream").unwrap();
        assert_eq!(upstream.resolved, "other");
    }

    #[test]
    fn test_should_have_empty_resolved_by_default() {
        let output = "origin\thttps://github.com/user/repo.git (fetch)";
        let remotes = Remote::parse_remotes(output);
        assert!(remotes[0].resolved.is_empty());
    }

    #[test]
    fn test_should_ignore_malformed_config_lines() {
        let output = "origin\thttps://github.com/user/repo.git (fetch)";
        let mut remotes = Remote::parse_remotes(output);
        Remote::populate_resolved(&mut remotes, "bad_line_no_space");
        assert!(remotes[0].resolved.is_empty());
    }

    // --- remote_sort_key tests ---

    #[test]
    fn test_should_sort_upstream_first() {
        assert!(remote_sort_key("upstream") < remote_sort_key("github"));
        assert!(remote_sort_key("github") < remote_sort_key("origin"));
        assert!(remote_sort_key("origin") < remote_sort_key("fork"));
        assert_eq!(remote_sort_key("fork"), remote_sort_key("myremote"));
    }
}
