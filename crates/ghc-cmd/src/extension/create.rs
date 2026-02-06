//! `ghc extension create` command.

use anyhow::{Context, Result};
use clap::Args;

use ghc_core::ios_eprintln;

/// Create a new extension scaffold.
#[derive(Debug, Args)]
pub struct CreateArgs {
    /// Name of the extension (will be prefixed with `gh-`).
    #[arg(value_name = "NAME")]
    name: String,

    /// Extension type.
    #[arg(long, value_parser = ["script", "go", "other"], default_value = "script")]
    kind: String,
}

impl CreateArgs {
    /// Run the extension create command.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension scaffold cannot be created.
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let ext_name = if self.name.starts_with("gh-") {
            self.name.clone()
        } else {
            format!("gh-{}", self.name)
        };

        let dir = std::path::Path::new(&ext_name);
        if dir.exists() {
            return Err(anyhow::anyhow!("directory {ext_name} already exists"));
        }

        tokio::fs::create_dir_all(&ext_name)
            .await
            .context("failed to create extension directory")?;

        match self.kind.as_str() {
            "script" => {
                let script_path = dir.join(&ext_name);
                let script_content =
                    format!("#!/usr/bin/env bash\nset -e\n\necho \"Hello from {ext_name}!\"\n");
                tokio::fs::write(&script_path, &script_content)
                    .await
                    .context("failed to write extension script")?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(0o755);
                    tokio::fs::set_permissions(&script_path, perms)
                        .await
                        .context("failed to set script permissions")?;
                }
            }
            "go" => {
                let main_go = dir.join("main.go");
                let go_content = format!(
                    "package main\n\nimport \"fmt\"\n\nfunc main() {{\n\tfmt.Println(\"Hello from {ext_name}!\")\n}}\n"
                );
                tokio::fs::write(&main_go, &go_content)
                    .await
                    .context("failed to write main.go")?;

                let go_mod = dir.join("go.mod");
                let mod_content = format!("module github.com/user/{ext_name}\n\ngo 1.21\n");
                tokio::fs::write(&go_mod, &mod_content)
                    .await
                    .context("failed to write go.mod")?;
            }
            _ => {
                let readme = dir.join("README.md");
                let readme_content = format!("# {ext_name}\n\nA GitHub CLI extension.\n");
                tokio::fs::write(&readme, &readme_content)
                    .await
                    .context("failed to write README.md")?;
            }
        }

        ios_eprintln!(
            ios,
            "{} Created extension {} (type: {})",
            cs.success_icon(),
            cs.bold(&ext_name),
            self.kind,
        );
        ios_eprintln!(ios, "  cd {ext_name} to get started");

        Ok(())
    }
}
