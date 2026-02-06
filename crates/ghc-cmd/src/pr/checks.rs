//! `ghc pr checks` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// GraphQL query for pull request status checks.
const PR_CHECKS_QUERY: &str = r"
query PullRequestChecks($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      statusCheckRollup: commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
              contexts(first: 100) {
                nodes {
                  __typename
                  ... on CheckRun {
                    name
                    status
                    conclusion
                    detailsUrl
                    startedAt
                    completedAt
                  }
                  ... on StatusContext {
                    context
                    state
                    targetUrl
                    createdAt
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
";

/// View CI status checks for a pull request.
#[derive(Debug, Args)]
pub struct ChecksArgs {
    /// Pull request number.
    #[arg(value_name = "NUMBER")]
    number: i64,

    /// Repository in OWNER/REPO format.
    #[arg(short = 'R', long)]
    repo: String,

    /// Watch for status changes (poll until all checks complete).
    #[arg(short, long)]
    watch: bool,

    /// Polling interval in seconds (used with --watch).
    #[arg(short, long, default_value = "10")]
    interval: u64,

    /// Fail if any required check fails.
    #[arg(long)]
    fail_fast: bool,

    /// Filter checks by name.
    #[arg(long)]
    required: bool,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,

    /// Filter JSON output using a jq expression.
    #[arg(short = 'q', long)]
    jq: Option<String>,

    /// Format JSON output using a Go template.
    #[arg(short = 't', long)]
    template: Option<String>,
}

impl ChecksArgs {
    /// Run the pr checks command.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or checks are not available.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let repo = ghc_core::repo::Repo::from_full_name(&self.repo)
            .context("invalid repository format")?;

        loop {
            let (all_complete, has_failures) = self.display_checks(factory, &repo).await?;

            if !self.watch || all_complete {
                if self.fail_fast && has_failures {
                    anyhow::bail!("one or more checks failed");
                }
                break;
            }

            ios_eprintln!(&factory.io, "\nWaiting for checks to complete...");
            tokio::time::sleep(std::time::Duration::from_secs(self.interval)).await;
        }

        Ok(())
    }

    /// Fetch and display check status. Returns (all_complete, has_failures).
    #[allow(clippy::too_many_lines)]
    async fn display_checks(
        &self,
        factory: &crate::factory::Factory,
        repo: &ghc_core::repo::Repo,
    ) -> Result<(bool, bool)> {
        let client = factory.api_client(repo.host())?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let mut variables = HashMap::new();
        variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
        variables.insert("name".to_string(), Value::String(repo.name().to_string()));
        variables.insert(
            "number".to_string(),
            Value::Number(serde_json::Number::from(self.number)),
        );

        let data: Value = client
            .graphql(PR_CHECKS_QUERY, &variables)
            .await
            .context("failed to fetch checks")?;

        let rollup_state = data
            .pointer(
                "/repository/pullRequest/statusCheckRollup/nodes/0/commit/statusCheckRollup/state",
            )
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");

        let contexts = data
            .pointer(
                "/repository/pullRequest/statusCheckRollup/nodes/0/commit/statusCheckRollup/contexts/nodes",
            )
            .and_then(Value::as_array);

        let Some(contexts) = contexts else {
            ios_eprintln!(ios, "No status checks found for PR #{}", self.number);
            return Ok((true, false));
        };

        if contexts.is_empty() {
            ios_eprintln!(ios, "No status checks found for PR #{}", self.number);
            return Ok((true, false));
        }

        // JSON output
        if !self.json.is_empty() || self.jq.is_some() || self.template.is_some() {
            let contexts_val = Value::Array(contexts.clone());
            let output = ghc_core::json::format_json_output(
                &contexts_val,
                &self.json,
                self.jq.as_deref(),
                self.template.as_deref(),
            )
            .context("failed to format JSON output")?;
            ios_println!(ios, "{output}");
            let all_complete = rollup_state != "PENDING";
            let has_failures = rollup_state == "FAILURE" || rollup_state == "ERROR";
            return Ok((all_complete, has_failures));
        }

        // Overall status
        let overall_display = match rollup_state {
            "SUCCESS" => cs.success("All checks passed"),
            "FAILURE" => cs.error("Some checks failed"),
            "ERROR" => cs.error("Some checks errored"),
            "PENDING" => cs.warning("Some checks are pending"),
            _ => rollup_state.to_string(),
        };
        ios_eprintln!(ios, "{overall_display}");

        // Table of individual checks
        let mut tp = TablePrinter::new(ios);
        let mut all_complete = true;
        let mut has_failures = false;

        for context in contexts {
            let type_name = context
                .get("__typename")
                .and_then(Value::as_str)
                .unwrap_or("");

            let (name, status_text, url) = if type_name == "CheckRun" {
                let name = context.get("name").and_then(Value::as_str).unwrap_or("");
                let status = context.get("status").and_then(Value::as_str).unwrap_or("");
                let conclusion = context
                    .get("conclusion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let url = context
                    .get("detailsUrl")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                let status_text = if status == "COMPLETED" {
                    match conclusion {
                        "SUCCESS" => cs.success(&format!("{} pass", cs.success_icon())),
                        "FAILURE" => {
                            has_failures = true;
                            cs.error(&format!("{} fail", cs.error_icon()))
                        }
                        "NEUTRAL" | "SKIPPED" => cs.gray("skip"),
                        _ => {
                            has_failures = true;
                            cs.error(conclusion)
                        }
                    }
                } else {
                    all_complete = false;
                    cs.warning("pending")
                };

                (name.to_string(), status_text, url.to_string())
            } else {
                // StatusContext
                let name = context.get("context").and_then(Value::as_str).unwrap_or("");
                let state = context.get("state").and_then(Value::as_str).unwrap_or("");
                let url = context
                    .get("targetUrl")
                    .and_then(Value::as_str)
                    .unwrap_or("");

                let status_text = match state {
                    "SUCCESS" => cs.success(&format!("{} pass", cs.success_icon())),
                    "FAILURE" | "ERROR" => {
                        has_failures = true;
                        cs.error(&format!("{} fail", cs.error_icon()))
                    }
                    "PENDING" | "EXPECTED" => {
                        all_complete = false;
                        cs.warning("pending")
                    }
                    _ => state.to_string(),
                };

                (name.to_string(), status_text, url.to_string())
            };

            tp.add_row(vec![status_text, name, url]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok((all_complete, has_failures))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{TestHarness, mock_graphql};

    fn checks_response(state: &str, contexts: &[serde_json::Value]) -> serde_json::Value {
        serde_json::json!({
            "data": {
                "repository": {
                    "pullRequest": {
                        "statusCheckRollup": {
                            "nodes": [{
                                "commit": {
                                    "statusCheckRollup": {
                                        "state": state,
                                        "contexts": {
                                            "nodes": contexts
                                        }
                                    }
                                }
                            }]
                        }
                    }
                }
            }
        })
    }

    #[tokio::test]
    async fn test_should_display_passing_checks() {
        let h = TestHarness::new().await;
        let contexts = vec![serde_json::json!({
            "__typename": "CheckRun",
            "name": "CI / build",
            "status": "COMPLETED",
            "conclusion": "SUCCESS",
            "detailsUrl": "https://github.com/owner/repo/actions/runs/1",
            "startedAt": "2024-01-15T10:00:00Z",
            "completedAt": "2024-01-15T10:05:00Z"
        })];

        mock_graphql(
            &h.server,
            "PullRequestChecks",
            checks_response("SUCCESS", &contexts),
        )
        .await;

        let args = ChecksArgs {
            number: 30,
            repo: "owner/repo".into(),
            watch: false,
            interval: 10,
            fail_fast: false,
            required: false,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("passed"),
            "should show all checks passed: {err}"
        );
    }

    #[tokio::test]
    async fn test_should_display_no_checks_found() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "PullRequestChecks",
            checks_response("UNKNOWN", &[]),
        )
        .await;

        let args = ChecksArgs {
            number: 31,
            repo: "owner/repo".into(),
            watch: false,
            interval: 10,
            fail_fast: false,
            required: false,
            json: vec![],
            jq: None,
            template: None,
        };

        args.run(&h.factory).await.unwrap();
        let err = h.stderr();
        assert!(
            err.contains("No status checks"),
            "should report no checks: {err}",
        );
    }

    #[tokio::test]
    async fn test_should_fail_fast_on_check_failure() {
        let h = TestHarness::new().await;
        let contexts = vec![serde_json::json!({
            "__typename": "CheckRun",
            "name": "CI / test",
            "status": "COMPLETED",
            "conclusion": "FAILURE",
            "detailsUrl": "https://example.com",
            "startedAt": "2024-01-15T10:00:00Z",
            "completedAt": "2024-01-15T10:05:00Z"
        })];

        mock_graphql(
            &h.server,
            "PullRequestChecks",
            checks_response("FAILURE", &contexts),
        )
        .await;

        let args = ChecksArgs {
            number: 32,
            repo: "owner/repo".into(),
            watch: false,
            interval: 10,
            fail_fast: true,
            required: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("checks failed"));
    }

    #[tokio::test]
    async fn test_should_return_error_on_invalid_repo_for_checks() {
        let h = TestHarness::new().await;
        let args = ChecksArgs {
            number: 1,
            repo: "bad".into(),
            watch: false,
            interval: 10,
            fail_fast: false,
            required: false,
            json: vec![],
            jq: None,
            template: None,
        };

        let result = args.run(&h.factory).await;
        assert!(result.is_err());
    }
}
