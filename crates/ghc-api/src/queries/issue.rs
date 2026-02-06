//! Issue-related API queries.

use serde::{Deserialize, Serialize};

/// Issue metadata from the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Issue number.
    pub number: i64,
    /// Title.
    pub title: String,
    /// Body text.
    pub body: Option<String>,
    /// State (OPEN, CLOSED).
    pub state: String,
    /// Author info.
    pub author: Option<Actor>,
    /// Labels.
    pub labels: Option<LabelConnection>,
    /// Assignees.
    pub assignees: Option<UserConnection>,
    /// URL.
    pub url: String,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: Option<String>,
    /// Closed at.
    pub closed_at: Option<String>,
    /// Number of comments.
    pub comments: Option<CommentCount>,
}

/// Actor (user who performed an action).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    /// Login name.
    pub login: String,
}

/// Labels connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelConnection {
    /// Label nodes.
    pub nodes: Vec<Label>,
}

/// A single label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    /// Label name.
    pub name: String,
    /// Label color (hex).
    pub color: String,
}

/// User connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConnection {
    /// User nodes.
    pub nodes: Vec<Actor>,
}

/// Comment count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentCount {
    /// Total count.
    pub total_count: i64,
}

/// GraphQL query for listing issues.
pub const ISSUE_LIST_QUERY: &str = r"
query IssueList($owner: String!, $name: String!, $first: Int!, $after: String, $states: [IssueState!], $labels: [String!], $assignee: String) {
  repository(owner: $owner, name: $name) {
    issues(first: $first, after: $after, states: $states, labels: $labels, filterBy: {assignee: $assignee}, orderBy: {field: CREATED_AT, direction: DESC}) {
      pageInfo {
        hasNextPage
        endCursor
      }
      nodes {
        number
        title
        state
        author { login }
        labels(first: 10) { nodes { name color } }
        assignees(first: 5) { nodes { login } }
        url
        createdAt
        updatedAt
        comments { totalCount }
      }
    }
  }
}
";

/// GraphQL query for viewing a single issue.
pub const ISSUE_VIEW_QUERY: &str = r"
query IssueView($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issue(number: $number) {
      number
      title
      body
      state
      author { login }
      labels(first: 20) { nodes { name color } }
      assignees(first: 10) { nodes { login } }
      url
      createdAt
      updatedAt
      closedAt
      comments { totalCount }
    }
  }
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_deserialize_issue() {
        let json = r#"{
            "number": 42,
            "title": "Bug report",
            "state": "OPEN",
            "author": {"login": "user"},
            "url": "https://github.com/cli/cli/issues/42",
            "createdAt": "2024-01-01T00:00:00Z"
        }"#;
        let issue: Issue = serde_json::from_str(json).unwrap();
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Bug report");
        assert_eq!(issue.state, "OPEN");
        assert_eq!(issue.author.as_ref().unwrap().login, "user");
    }

    #[test]
    fn test_should_deserialize_issue_with_labels() {
        let json = r#"{
            "number": 1,
            "title": "Test",
            "state": "OPEN",
            "url": "https://github.com/o/r/issues/1",
            "createdAt": "2024-01-01T00:00:00Z",
            "labels": {"nodes": [{"name": "bug", "color": "d73a4a"}]}
        }"#;
        let issue: Issue = serde_json::from_str(json).unwrap();
        let labels = issue.labels.unwrap();
        assert_eq!(labels.nodes.len(), 1);
        assert_eq!(labels.nodes[0].name, "bug");
        assert_eq!(labels.nodes[0].color, "d73a4a");
    }

    #[test]
    fn test_should_deserialize_comment_count() {
        let json = r#"{"totalCount": 5}"#;
        let count: CommentCount = serde_json::from_str(json).unwrap();
        assert_eq!(count.total_count, 5);
    }

    #[test]
    fn test_should_contain_issue_list_query_fields() {
        assert!(ISSUE_LIST_QUERY.contains("IssueList"));
        assert!(ISSUE_LIST_QUERY.contains("number"));
        assert!(ISSUE_LIST_QUERY.contains("title"));
        assert!(ISSUE_LIST_QUERY.contains("pageInfo"));
    }
}
