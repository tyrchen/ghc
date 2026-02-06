//! `ghc project edit` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::{ios_eprintln, ios_println};
use serde_json::Value;

/// Edit a project.
#[derive(Debug, Args)]
pub struct EditArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// New title for the project.
    #[arg(long)]
    title: Option<String>,

    /// New short description for the project.
    #[arg(long)]
    description: Option<String>,

    /// Visibility of the project (PUBLIC or PRIVATE).
    #[arg(long)]
    visibility: Option<String>,

    /// New README content for the project.
    #[arg(long)]
    readme: Option<String>,
}

impl EditArgs {
    /// Run the project edit command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be edited.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = r"
            mutation EditProject(
                $projectId: ID!,
                $title: String,
                $shortDescription: String,
                $public: Boolean,
                $readme: String
            ) {
                updateProjectV2(input: {
                    projectId: $projectId,
                    title: $title,
                    shortDescription: $shortDescription,
                    public: $public,
                    readme: $readme
                }) {
                    projectV2 { id title url }
                }
            }
        ";

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));

        if let Some(title) = &self.title {
            vars.insert("title".to_string(), Value::String(title.clone()));
        }
        if let Some(desc) = &self.description {
            vars.insert("shortDescription".to_string(), Value::String(desc.clone()));
        }
        if let Some(vis) = &self.visibility {
            let is_public = vis.eq_ignore_ascii_case("PUBLIC");
            vars.insert("public".to_string(), Value::Bool(is_public));
        }
        if let Some(readme) = &self.readme {
            vars.insert("readme".to_string(), Value::String(readme.clone()));
        }

        let result: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to edit project")?;

        let url = result
            .pointer("/updateProjectV2/projectV2/url")
            .and_then(Value::as_str)
            .unwrap_or("");

        let ios = &factory.io;
        let cs = ios.color_scheme();
        ios_eprintln!(ios, "{} Edited project #{}", cs.success_icon(), self.number,);
        if !url.is_empty() {
            ios_println!(ios, "{url}");
        }

        Ok(())
    }
}
