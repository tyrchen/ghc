//! `ghc extension create` command.

use std::path::Path;

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

    /// Create a precompiled extension with release workflow.
    ///
    /// Valid values: "go" or "other". When set, generates a GitHub Actions
    /// workflow for building and releasing platform-specific binaries.
    #[arg(long, value_parser = ["go", "other"])]
    precompiled: Option<String>,
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

        let dir = Path::new(&ext_name);
        if dir.exists() {
            return Err(anyhow::anyhow!("directory {ext_name} already exists"));
        }

        tokio::fs::create_dir_all(&ext_name)
            .await
            .context("failed to create extension directory")?;

        let effective_kind = self.precompiled.as_deref().unwrap_or(self.kind.as_str());

        self.scaffold_files(dir, &ext_name, effective_kind).await?;
        self.init_git_repo(ios, dir, &ext_name).await;

        let kind_label = if self.precompiled.is_some() {
            format!("{effective_kind}, precompiled")
        } else {
            effective_kind.to_string()
        };

        ios_eprintln!(
            ios,
            "{} Created extension {} (type: {kind_label})",
            cs.success_icon(),
            cs.bold(&ext_name),
        );
        ios_eprintln!(ios, "  cd {ext_name} to get started");

        Ok(())
    }

    /// Create scaffold files based on extension kind.
    async fn scaffold_files(&self, dir: &Path, ext_name: &str, kind: &str) -> Result<()> {
        match kind {
            "script" => self.scaffold_script(dir, ext_name).await,
            "go" => self.scaffold_go(dir, ext_name).await,
            "other" => self.scaffold_other(dir, ext_name).await,
            _ => Err(anyhow::anyhow!("unknown extension kind: {kind}")),
        }
    }

    /// Create scaffold for a script extension.
    async fn scaffold_script(&self, dir: &Path, ext_name: &str) -> Result<()> {
        let script_path = dir.join(ext_name);
        let content = format!("#!/usr/bin/env bash\nset -e\n\necho \"Hello from {ext_name}!\"\n");
        tokio::fs::write(&script_path, &content)
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

        Ok(())
    }

    /// Create scaffold for a Go extension.
    async fn scaffold_go(&self, dir: &Path, ext_name: &str) -> Result<()> {
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

        if self.precompiled.is_some() {
            write_release_workflow(dir, ext_name, "go").await?;
        }

        Ok(())
    }

    /// Create scaffold for a generic precompiled extension.
    async fn scaffold_other(&self, dir: &Path, ext_name: &str) -> Result<()> {
        let readme = dir.join("README.md");
        let readme_content = format!(
            "# {ext_name}\n\nA GitHub CLI precompiled extension.\n\n\
             Build your extension binary and create releases with platform-specific assets.\n"
        );
        tokio::fs::write(&readme, &readme_content)
            .await
            .context("failed to write README.md")?;

        let makefile = dir.join("Makefile");
        let makefile_content = format!(
            ".PHONY: build clean\n\nBINARY := {ext_name}\n\n\
             build:\n\t@echo \"Build your extension here\"\n\n\
             clean:\n\trm -f $(BINARY)\n"
        );
        tokio::fs::write(&makefile, &makefile_content)
            .await
            .context("failed to write Makefile")?;

        if self.precompiled.is_some() {
            write_release_workflow(dir, ext_name, "other").await?;
        }

        Ok(())
    }

    /// Initialize a git repository and create an initial commit.
    async fn init_git_repo(
        &self,
        ios: &ghc_core::iostreams::IOStreams,
        dir: &Path,
        ext_name: &str,
    ) {
        let git_init = tokio::process::Command::new("git")
            .args(["init", ext_name])
            .output()
            .await;

        let Ok(output) = git_init else {
            ios_eprintln!(ios, "Warning: git init failed");
            return;
        };

        if output.status.success() {
            let _ = tokio::process::Command::new("git")
                .args(["add", "."])
                .current_dir(dir)
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .args(["commit", "-m", "Initial commit", "--allow-empty"])
                .current_dir(dir)
                .output()
                .await;
        } else {
            ios_eprintln!(
                ios,
                "Warning: git init failed ({})",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
}

/// Write a GitHub Actions release workflow for precompiled extensions.
async fn write_release_workflow(dir: &Path, ext_name: &str, kind: &str) -> Result<()> {
    let workflow_dir = dir.join(".github").join("workflows");
    tokio::fs::create_dir_all(&workflow_dir)
        .await
        .context("failed to create .github/workflows directory")?;

    let build_step = if kind == "go" {
        format!(
            r"      - uses: actions/setup-go@v4
        with:
          go-version: '1.21'
      - name: Build
        run: |
          GOOS=${{{{ matrix.os }}}} GOARCH=${{{{ matrix.arch }}}} go build -o {ext_name}-${{{{ matrix.os }}}}-${{{{ matrix.arch }}}} .
      - name: Upload
        uses: actions/upload-artifact@v3
        with:
          name: {ext_name}-${{{{ matrix.os }}}}-${{{{ matrix.arch }}}}
          path: {ext_name}-${{{{ matrix.os }}}}-${{{{ matrix.arch }}}}"
        )
    } else {
        format!(
            r"      - name: Build
        run: make build
      - name: Upload
        uses: actions/upload-artifact@v3
        with:
          name: {ext_name}-${{{{ matrix.os }}}}-${{{{ matrix.arch }}}}
          path: {ext_name}"
        )
    };

    let workflow_content = format!(
        r"name: Release
on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        include:
          - os: linux
            arch: amd64
          - os: linux
            arch: arm64
          - os: darwin
            arch: amd64
          - os: darwin
            arch: arm64
          - os: windows
            arch: amd64
    steps:
      - uses: actions/checkout@v4
{build_step}

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v3
      - uses: softprops/action-gh-release@v1
        with:
          files: '{ext_name}-*/*'
"
    );

    let workflow_path = workflow_dir.join("release.yml");
    tokio::fs::write(&workflow_path, &workflow_content)
        .await
        .context("failed to write release workflow")?;

    Ok(())
}
