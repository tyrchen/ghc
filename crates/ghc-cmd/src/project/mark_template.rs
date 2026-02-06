//! `ghc project mark-template` command.

use std::collections::HashMap;

use anyhow::{Context, Result};
use clap::Args;
use ghc_core::ios_eprintln;
use serde_json::Value;

/// Mark or unmark a project as a template.
#[derive(Debug, Args)]
pub struct MarkTemplateArgs {
    /// Project number.
    #[arg(value_name = "NUMBER")]
    number: u32,

    /// Owner of the project (user or organization).
    #[arg(long)]
    owner: String,

    /// Undo: unmark the project as a template.
    #[arg(long)]
    undo: bool,
}

impl MarkTemplateArgs {
    /// Run the project mark-template command.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be marked as a template.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let client = factory.api_client("github.com")?;

        let project_id =
            super::close::resolve_project_id(&client, &self.owner, self.number).await?;

        let query = if self.undo {
            r"
                mutation UnmarkTemplate($projectId: ID!) {
                    unmarkProjectV2AsTemplate(input: { projectId: $projectId }) {
                        projectV2 { id title }
                    }
                }
            "
        } else {
            r"
                mutation MarkTemplate($projectId: ID!) {
                    markProjectV2AsTemplate(input: { projectId: $projectId }) {
                        projectV2 { id title }
                    }
                }
            "
        };

        let mut vars = HashMap::new();
        vars.insert("projectId".to_string(), Value::String(project_id));

        let _: Value = client
            .graphql(query, &vars)
            .await
            .context("failed to update template status")?;

        let ios = &factory.io;
        let cs = ios.color_scheme();
        if self.undo {
            ios_eprintln!(
                ios,
                "{} Unmarked project #{} as template",
                cs.success_icon(),
                self.number,
            );
        } else {
            ios_eprintln!(
                ios,
                "{} Marked project #{} as template",
                cs.success_icon(),
                self.number,
            );
        }

        Ok(())
    }
}
