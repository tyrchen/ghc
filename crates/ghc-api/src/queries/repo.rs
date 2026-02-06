//! Repository-related API queries.

use serde::{Deserialize, Serialize};

/// Repository metadata from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    /// Repository name.
    pub name: String,
    /// Owner login.
    pub owner: OwnerInfo,
    /// Description.
    pub description: Option<String>,
    /// URL.
    pub url: String,
    /// SSH URL.
    pub ssh_url: Option<String>,
    /// Whether the repo is a fork.
    pub is_fork: bool,
    /// Whether the repo is archived.
    pub is_archived: bool,
    /// Whether the repo is private.
    pub is_private: bool,
    /// Default branch name.
    pub default_branch_ref: Option<BranchRef>,
    /// Parent repo (if fork).
    pub parent: Option<Box<Repository>>,
    /// Star count.
    pub stargazer_count: Option<i64>,
    /// Fork count.
    pub fork_count: Option<i64>,
    /// Primary language.
    pub primary_language: Option<Language>,
    /// Created at timestamp.
    pub created_at: Option<String>,
    /// Updated at timestamp.
    pub updated_at: Option<String>,
}

/// Repository owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerInfo {
    /// Login name.
    pub login: String,
}

/// Branch reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRef {
    /// Branch name.
    pub name: String,
}

/// Programming language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Language {
    /// Language name.
    pub name: String,
}

/// GraphQL query for repository metadata.
pub const REPO_QUERY: &str = r"
query RepositoryInfo($owner: String!, $name: String!) {
  repository(owner: $owner, name: $name) {
    name
    owner { login }
    description
    url
    sshUrl
    isFork
    isArchived
    isPrivate
    defaultBranchRef { name }
    parent {
      name
      owner { login }
      url
    }
    stargazerCount
    forkCount
    primaryLanguage { name }
    createdAt
    updatedAt
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_deserialize_repository() {
        let json = r#"{
            "name": "cli",
            "owner": {"login": "cli"},
            "url": "https://github.com/cli/cli",
            "isFork": false,
            "isArchived": false,
            "isPrivate": false
        }"#;
        let repo: Repository = serde_json::from_str(json).unwrap();
        assert_eq!(repo.name, "cli");
        assert_eq!(repo.owner.login, "cli");
        assert!(!repo.is_fork);
        assert!(!repo.is_archived);
        assert!(!repo.is_private);
    }

    #[test]
    fn test_should_deserialize_repository_with_all_fields() {
        let json = r#"{
            "name": "repo",
            "owner": {"login": "org"},
            "description": "A great repo",
            "url": "https://github.com/org/repo",
            "sshUrl": "git@github.com:org/repo.git",
            "isFork": true,
            "isArchived": false,
            "isPrivate": true,
            "defaultBranchRef": {"name": "main"},
            "stargazerCount": 1000,
            "forkCount": 50,
            "primaryLanguage": {"name": "Rust"},
            "createdAt": "2023-01-01T00:00:00Z"
        }"#;
        let repo: Repository = serde_json::from_str(json).unwrap();
        assert_eq!(repo.description, Some("A great repo".to_string()));
        assert!(repo.is_fork);
        assert!(repo.is_private);
        assert_eq!(repo.default_branch_ref.unwrap().name, "main");
        assert_eq!(repo.stargazer_count, Some(1000));
        assert_eq!(repo.fork_count, Some(50));
        assert_eq!(repo.primary_language.unwrap().name, "Rust");
    }

    #[test]
    fn test_should_contain_repo_query_fields() {
        assert!(REPO_QUERY.contains("RepositoryInfo"));
        assert!(REPO_QUERY.contains("name"));
        assert!(REPO_QUERY.contains("owner"));
        assert!(REPO_QUERY.contains("defaultBranchRef"));
    }
}
