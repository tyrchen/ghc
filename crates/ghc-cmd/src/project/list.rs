//! `ghc project list` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use serde_json::Value;

use ghc_core::table::TablePrinter;
use ghc_core::{ios_eprintln, ios_println};

/// List projects for an owner.
#[derive(Debug, Args)]
pub struct ListArgs {
    /// Owner of the projects (user or organization).
    #[arg(long)]
    owner: String,

    /// Include closed projects.
    #[arg(long)]
    closed: bool,

    /// Maximum number of projects to fetch.
    #[arg(short = 'L', long, default_value = "30")]
    limit: u32,

    /// Output JSON with specified fields.
    #[arg(long, value_delimiter = ',')]
    json: Vec<String>,
}

impl ListArgs {
    /// Run the project list command.
    ///
    /// # Errors
    ///
    /// Returns an error if the projects cannot be listed.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let query = r"
            query ListProjects($owner: String!, $first: Int!) {
                user(login: $owner) {
                    projectsV2(first: $first) {
                        nodes { number title closed url shortDescription }
                    }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("owner".to_string(), Value::String(self.owner.clone()));
        vars.insert(
            "first".to_string(),
            Value::Number(serde_json::Number::from(self.limit)),
        );

        let data: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to list projects")?;

        let projects = data
            .pointer("/user/projectsV2/nodes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        // If user query returned no results, try organization
        let projects = if projects.is_empty() {
            let org_query = r"
                query ListOrgProjects($owner: String!, $first: Int!) {
                    organization(login: $owner) {
                        projectsV2(first: $first) {
                            nodes { number title closed url shortDescription }
                        }
                    }
                }
            ";

            let org_data: Value = client
                .graphql(org_query, &vars)
                .await
                .context("failed to list organization projects")?;

            org_data
                .pointer("/organization/projectsV2/nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        } else {
            projects
        };

        // Filter closed projects unless --closed is set
        let projects: Vec<&Value> = projects
            .iter()
            .filter(|p| self.closed || !p.get("closed").and_then(Value::as_bool).unwrap_or(false))
            .collect();

        let ios = &factory.io;
        let cs = ios.color_scheme();

        if !self.json.is_empty() {
            ios_println!(ios, "{}", serde_json::to_string_pretty(&projects)?);
            return Ok(());
        }

        if projects.is_empty() {
            ios_eprintln!(ios, "No projects found for {}", self.owner);
            return Ok(());
        }
        let mut tp = TablePrinter::new(ios);

        for project in &projects {
            let number = project.get("number").and_then(Value::as_u64).unwrap_or(0);
            let title = project.get("title").and_then(Value::as_str).unwrap_or("");
            let closed = project
                .get("closed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let url = project.get("url").and_then(Value::as_str).unwrap_or("");

            let status = if closed {
                cs.gray("closed")
            } else {
                cs.success("open")
            };

            tp.add_row(vec![
                format!("#{number}"),
                cs.bold(title),
                status,
                cs.gray(url),
            ]);
        }

        let output = tp.render();
        ios_println!(ios, "{output}");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql};

    #[tokio::test]
    async fn test_should_list_user_projects() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "ListProjects",
            serde_json::json!({
                "data": {
                    "user": {
                        "projectsV2": {
                            "nodes": [
                                {
                                    "number": 1,
                                    "title": "My Roadmap",
                                    "closed": false,
                                    "url": "https://github.com/users/testuser/projects/1",
                                    "shortDescription": "A project"
                                },
                                {
                                    "number": 2,
                                    "title": "Archive",
                                    "closed": true,
                                    "url": "https://github.com/users/testuser/projects/2",
                                    "shortDescription": "Old project"
                                }
                            ]
                        }
                    }
                }
            }),
        )
        .await;

        let args = ListArgs {
            owner: "testuser".to_string(),
            closed: false,
            limit: 30,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(
            stdout.contains("My Roadmap"),
            "should contain open project title"
        );
        assert!(
            !stdout.contains("Archive"),
            "should not contain closed project when --closed is not set"
        );
    }

    #[tokio::test]
    async fn test_should_include_closed_projects_when_flag_set() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "ListProjects",
            serde_json::json!({
                "data": {
                    "user": {
                        "projectsV2": {
                            "nodes": [
                                {
                                    "number": 1,
                                    "title": "My Roadmap",
                                    "closed": false,
                                    "url": "https://github.com/users/testuser/projects/1",
                                    "shortDescription": "A project"
                                },
                                {
                                    "number": 2,
                                    "title": "Archive",
                                    "closed": true,
                                    "url": "https://github.com/users/testuser/projects/2",
                                    "shortDescription": "Old project"
                                }
                            ]
                        }
                    }
                }
            }),
        )
        .await;

        let args = ListArgs {
            owner: "testuser".to_string(),
            closed: true,
            limit: 30,
            json: vec![],
        };
        args.run(&h.factory).await.unwrap();

        let stdout = h.stdout();
        assert!(stdout.contains("My Roadmap"), "should contain open project");
        assert!(
            stdout.contains("Archive"),
            "should contain closed project when --closed is set"
        );
    }
}
