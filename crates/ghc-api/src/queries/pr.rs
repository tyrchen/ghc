//! Pull request-related API queries.

use serde::{Deserialize, Serialize};

use super::issue::{Actor, CommentCount, LabelConnection};

/// Pull request metadata from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// PR number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body text.
    pub body: Option<String>,
    /// State (OPEN, CLOSED, MERGED).
    pub state: String,
    /// Whether it's a draft.
    pub is_draft: bool,
    /// Author.
    pub author: Option<Actor>,
    /// Head branch ref.
    pub head_ref_name: String,
    /// Base branch ref.
    pub base_ref_name: String,
    /// Labels.
    pub labels: Option<LabelConnection>,
    /// URL.
    pub url: String,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: Option<String>,
    /// Merged at.
    pub merged_at: Option<String>,
    /// Closed at.
    pub closed_at: Option<String>,
    /// Number of comments.
    pub comments: Option<CommentCount>,
    /// Additions.
    pub additions: Option<i64>,
    /// Deletions.
    pub deletions: Option<i64>,
    /// Changed files.
    pub changed_files: Option<i64>,
    /// Review decision.
    pub review_decision: Option<String>,
    /// Mergeable state.
    pub mergeable: Option<String>,
}

/// GraphQL query for listing pull requests.
pub const PR_LIST_QUERY: &str = r"
query PullRequestList($owner: String!, $name: String!, $first: Int!, $after: String, $states: [PullRequestState!], $labels: [String!], $headRefName: String, $baseRefName: String) {
  repository(owner: $owner, name: $name) {
    pullRequests(first: $first, after: $after, states: $states, labels: $labels, headRefName: $headRefName, baseRefName: $baseRefName, orderBy: {field: CREATED_AT, direction: DESC}) {
      pageInfo {
        hasNextPage
        endCursor
      }
      nodes {
        number
        title
        state
        isDraft
        author { login }
        headRefName
        baseRefName
        labels(first: 10) { nodes { name color } }
        url
        createdAt
        updatedAt
        comments { totalCount }
        additions
        deletions
        changedFiles
        reviewDecision
      }
    }
  }
}
";

/// GraphQL query for viewing a single pull request.
pub const PR_VIEW_QUERY: &str = r"
query PullRequestView($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number
      title
      body
      state
      isDraft
      author { login }
      headRefName
      baseRefName
      labels(first: 20) { nodes { name color } }
      url
      createdAt
      updatedAt
      mergedAt
      closedAt
      comments { totalCount }
      additions
      deletions
      changedFiles
      reviewDecision
      mergeable
    }
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_deserialize_pull_request() {
        let json = r#"{
            "number": 123,
            "title": "Add feature",
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "feature-branch",
            "baseRefName": "main",
            "url": "https://github.com/cli/cli/pull/123",
            "createdAt": "2024-01-01T00:00:00Z"
        }"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.number, 123);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.state, "OPEN");
        assert!(!pr.is_draft);
        assert_eq!(pr.head_ref_name, "feature-branch");
        assert_eq!(pr.base_ref_name, "main");
    }

    #[test]
    fn test_should_deserialize_pr_with_optional_fields() {
        let json = r#"{
            "number": 1,
            "title": "Fix",
            "state": "MERGED",
            "isDraft": false,
            "headRefName": "fix",
            "baseRefName": "main",
            "url": "https://github.com/o/r/pull/1",
            "createdAt": "2024-01-01T00:00:00Z",
            "mergedAt": "2024-01-02T00:00:00Z",
            "additions": 10,
            "deletions": 5,
            "changedFiles": 3,
            "reviewDecision": "APPROVED",
            "mergeable": "MERGEABLE"
        }"#;
        let pr: PullRequest = serde_json::from_str(json).unwrap();
        assert_eq!(pr.state, "MERGED");
        assert_eq!(pr.merged_at, Some("2024-01-02T00:00:00Z".to_string()));
        assert_eq!(pr.additions, Some(10));
        assert_eq!(pr.deletions, Some(5));
        assert_eq!(pr.changed_files, Some(3));
        assert_eq!(pr.review_decision, Some("APPROVED".to_string()));
        assert_eq!(pr.mergeable, Some("MERGEABLE".to_string()));
    }

    #[test]
    fn test_should_contain_pr_list_query_fields() {
        assert!(PR_LIST_QUERY.contains("PullRequestList"));
        assert!(PR_LIST_QUERY.contains("number"));
        assert!(PR_LIST_QUERY.contains("isDraft"));
        assert!(PR_LIST_QUERY.contains("pageInfo"));
    }
}
