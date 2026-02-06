//! `ghc repo create` command.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use regex::Regex;
use serde_json::Value;

use ghc_core::iostreams::IOStreams;
use ghc_core::prompter::Prompter;
use ghc_core::repo::Repo;
use ghc_core::{ios_eprintln, ios_println};
use ghc_git::url_parser;

use crate::factory::Factory;

/// Maximum number of retries for clone operations after template creation.
const CLONE_MAX_RETRIES: u32 = 3;

/// Delay between clone retries.
const CLONE_RETRY_DELAY: Duration = Duration::from_secs(3);

/// Create a new repository.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct CreateArgs {
    /// Repository name (OWNER/REPO or just REPO for personal).
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Description of the repository.
    #[arg(short, long)]
    description: Option<String>,

    /// Repository home page URL.
    #[arg(short = 'H', long)]
    homepage: Option<String>,

    /// The name of the organization team to be granted access.
    #[arg(short, long)]
    team: Option<String>,

    /// Make the new repository based on a template repository.
    #[arg(short = 'p', long)]
    template: Option<String>,

    /// Make the repository public.
    #[arg(long, group = "visibility")]
    public: bool,

    /// Make the repository private.
    #[arg(long, group = "visibility")]
    private: bool,

    /// Make the repository internal.
    #[arg(long, group = "visibility")]
    internal: bool,

    /// Clone the new repository locally.
    #[arg(short, long)]
    clone: bool,

    /// Initialize with a README.
    #[arg(long)]
    add_readme: bool,

    /// License template (e.g., mit, apache-2.0).
    #[arg(short, long)]
    license: Option<String>,

    /// Gitignore template (e.g., Rust, Go).
    #[arg(short, long)]
    gitignore: Option<String>,

    /// Specify path to local repository to use as source.
    #[arg(short, long)]
    source: Option<String>,

    /// Specify remote name for the new repository.
    #[arg(short, long)]
    remote: Option<String>,

    /// Push local commits to the new repository.
    #[arg(long)]
    push: bool,

    /// Include all branches from template repository.
    #[arg(long)]
    include_all_branches: bool,

    /// Disable issues.
    #[arg(long)]
    disable_issues: bool,

    /// Disable wiki.
    #[arg(long)]
    disable_wiki: bool,
}

impl CreateArgs {
    /// Run the repo create command.
    pub async fn run(&self, factory: &Factory) -> Result<()> {
        self.validate_flags(factory)?;

        let is_interactive = self.name.is_none()
            && !self.public
            && !self.private
            && !self.internal
            && self.source.is_none()
            && self.template.is_none()
            && !self.clone
            && !self.push
            && !self.add_readme
            && self.description.is_none()
            && self.homepage.is_none()
            && self.team.is_none()
            && self.license.is_none()
            && self.gitignore.is_none()
            && self.remote.is_none()
            && !self.include_all_branches
            && !self.disable_issues
            && !self.disable_wiki;

        if is_interactive {
            if !factory.io.can_prompt() {
                bail!("at least one argument required in non-interactive mode");
            }
            return self.run_interactive(factory).await;
        }

        // Non-interactive mode requires visibility
        if !self.public && !self.private && !self.internal {
            bail!(
                "`--public`, `--private`, or `--internal` required when not running interactively"
            );
        }

        if self.source.is_some() {
            return self.create_from_local(factory, false).await;
        }

        self.create_from_scratch(factory, false).await
    }

    /// Validate flag combinations.
    fn validate_flags(&self, _factory: &Factory) -> Result<()> {
        if self.source.is_none() {
            if self.remote.is_some() {
                bail!("the `--remote` option can only be used with `--source`");
            }
            if self.push {
                bail!("the `--push` option can only be used with `--source`");
            }
        } else {
            if self.clone {
                bail!("the `--source` option is not supported with `--clone`");
            }
            if self.template.is_some() {
                bail!("the `--source` option is not supported with `--template`");
            }
            if self.license.is_some() {
                bail!("the `--source` option is not supported with `--license`");
            }
            if self.gitignore.is_some() {
                bail!("the `--source` option is not supported with `--gitignore`");
            }
        }

        if self.template.is_some() {
            if self.gitignore.is_some() || self.license.is_some() {
                bail!(".gitignore and license templates are not added when template is provided");
            }
            if self.add_readme {
                bail!("the `--add-readme` option is not supported with `--template`");
            }
            if self.team.is_some() {
                bail!("the `--template` option is not supported with `--team`");
            }
        }

        if self.template.is_none() && self.include_all_branches {
            bail!("the `--include-all-branches` option is only supported when using `--template`");
        }

        Ok(())
    }

    /// Run interactive mode with 3-way choice.
    async fn run_interactive(&self, factory: &Factory) -> Result<()> {
        let cfg_lock = factory.config()?;
        let host = {
            let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            cfg.authentication()
                .hosts()
                .into_iter()
                .next()
                .unwrap_or_else(|| "github.com".to_string())
        };

        let prompter = factory.prompter();
        let options = vec![
            format!("Create a new repository on {host} from scratch"),
            format!("Create a new repository on {host} from a template repository"),
            format!("Push an existing local repository to {host}"),
        ];
        let answer = prompter.select("What would you like to do?", None, &options)?;

        match answer {
            0 => self.create_from_scratch(factory, true).await,
            1 => self.create_from_template_interactive(factory).await,
            2 => self.create_from_local(factory, true).await,
            _ => bail!("unexpected selection"),
        }
    }

    /// Create a new repository from scratch.
    #[allow(clippy::too_many_lines)]
    async fn create_from_scratch(&self, factory: &Factory, interactive: bool) -> Result<()> {
        let cfg_lock = factory.config()?;
        let host = {
            let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            cfg.authentication()
                .hosts()
                .into_iter()
                .next()
                .unwrap_or_else(|| "github.com".to_string())
        };
        let client = factory.api_client(&host)?;
        let ios = &factory.io;
        let cs = ios.color_scheme();

        let (
            name,
            description,
            visibility,
            add_readme,
            gitignore_template,
            license_template,
            do_clone,
        ) = if interactive {
            let prompter = factory.prompter();
            let (name, desc, vis) =
                interactive_repo_info(&client, &host, prompter.as_ref(), "").await?;
            let readme = prompter.confirm("Would you like to add a README file?", false)?;
            let gi = interactive_gitignore(&client, &host, prompter.as_ref()).await?;
            let lic = interactive_license(&client, &host, prompter.as_ref()).await?;

            let target_repo = normalize_repo_name(&name);
            let display_name = if let Some(idx) = name.find('/') {
                format!(
                    "{}/{}",
                    &name[..=idx],
                    normalize_repo_name(&name[idx + 1..])
                )
            } else {
                target_repo.clone()
            };

            let confirmed = prompter.confirm(
                &format!(
                    "This will create \"{}\" as a {} repository on {}. Continue?",
                    display_name,
                    vis.to_lowercase(),
                    host,
                ),
                true,
            )?;
            if !confirmed {
                bail!("cancelled");
            }

            let should_clone = prompter.confirm("Clone the new repository locally?", true)?;
            (name, desc, vis, readme, gi, lic, should_clone)
        } else {
            let name = self
                .name
                .as_deref()
                .ok_or_else(|| {
                    anyhow::anyhow!("name argument required to create new remote repository")
                })?
                .to_string();
            let vis = if self.public {
                "PUBLIC".to_string()
            } else if self.internal {
                "INTERNAL".to_string()
            } else {
                "PRIVATE".to_string()
            };
            (
                name,
                self.description.clone().unwrap_or_default(),
                vis,
                self.add_readme,
                self.gitignore.clone().unwrap_or_default(),
                self.license.clone().unwrap_or_default(),
                self.clone,
            )
        };

        // Parse owner/name
        let (owner, repo_name) = if name.contains('/') {
            let repo =
                Repo::from_full_name(&name).map_err(|e| anyhow::anyhow!("argument error: {e}"))?;
            (repo.owner().to_string(), repo.name().to_string())
        } else {
            (String::new(), name.clone())
        };

        // Build create input
        let mut template_repo_main_branch = String::new();
        let template_repo_id = if let Some(ref tmpl) = self.template {
            let (id, branch) = resolve_template_repo(&client, &host, tmpl).await?;
            template_repo_main_branch = branch;
            Some(id)
        } else {
            None
        };

        let repo = create_repo(
            &client,
            &host,
            &RepoCreateInput {
                name: repo_name.clone(),
                visibility: visibility.clone(),
                owner_login: owner.clone(),
                team_slug: self.team.clone().unwrap_or_default(),
                description: description.clone(),
                homepage_url: self.homepage.clone().unwrap_or_default(),
                has_issues_enabled: !self.disable_issues,
                has_wiki_enabled: !self.disable_wiki,
                gitignore_template: gitignore_template.clone(),
                license_template: license_template.clone(),
                include_all_branches: self.include_all_branches,
                init_readme: add_readme,
                template_repository_id: template_repo_id.unwrap_or_default(),
            },
        )
        .await?;

        let full_name = repo
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(&repo_name);
        let html_url = repo.get("html_url").and_then(Value::as_str).unwrap_or("");

        if ios.is_stdout_tty() {
            ios_eprintln!(
                ios,
                "{} Created repository {} on {}",
                cs.success_icon(),
                full_name,
                host,
            );
            ios_eprintln!(ios, "  {html_url}");
        } else {
            ios_println!(ios, "{html_url}");
        }

        if do_clone {
            let protocol = {
                let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                cfg.git_protocol(&host)
            };
            let clone_owner = repo
                .pointer("/owner/login")
                .and_then(Value::as_str)
                .unwrap_or(&owner);
            let clone_name = repo
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&repo_name);
            let repo_ref = Repo::with_host(clone_owner, clone_name, &host);
            let remote_url = url_parser::clone_url(&repo_ref, &protocol);

            let has_content = add_readme
                || !license_template.is_empty()
                || !gitignore_template.is_empty()
                || self.template.is_some();

            if has_content {
                clone_with_retry(&remote_url, &template_repo_main_branch).await?;
            } else {
                local_init(&remote_url, clone_name).await?;
            }
        }

        Ok(())
    }

    /// Create a new repository from a template (interactive only).
    #[allow(clippy::too_many_lines)]
    async fn create_from_template_interactive(&self, factory: &Factory) -> Result<()> {
        let cfg_lock = factory.config()?;
        let host = {
            let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            cfg.authentication()
                .hosts()
                .into_iter()
                .next()
                .unwrap_or_else(|| "github.com".to_string())
        };
        let client = factory.api_client(&host)?;
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let prompter = factory.prompter();

        let (name, description, visibility) =
            interactive_repo_info(&client, &host, prompter.as_ref(), "").await?;

        // Ensure we have an owner for template lookup
        let full_name = if name.contains('/') {
            name.clone()
        } else {
            let username = client
                .current_login()
                .await
                .context("failed to get current user")?;
            format!("{username}/{name}")
        };
        let repo_ref =
            Repo::from_full_name(&full_name).map_err(|e| anyhow::anyhow!("argument error: {e}"))?;
        let owner = repo_ref.owner().to_string();
        let repo_name = repo_ref.name().to_string();

        // Select template
        let template_repo =
            interactive_repo_template(&client, &host, &owner, prompter.as_ref()).await?;
        let template_id = template_repo
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let template_main_branch = template_repo
            .pointer("/defaultBranchRef/name")
            .and_then(Value::as_str)
            .unwrap_or("main")
            .to_string();

        let target_repo = normalize_repo_name(&full_name);
        let display_name = if let Some(idx) = full_name.find('/') {
            format!(
                "{}/{}",
                &full_name[..=idx],
                normalize_repo_name(&full_name[idx + 1..])
            )
        } else {
            target_repo
        };

        let confirmed = prompter.confirm(
            &format!(
                "This will create \"{}\" as a {} repository on {}. Continue?",
                display_name,
                visibility.to_lowercase(),
                host,
            ),
            true,
        )?;
        if !confirmed {
            bail!("cancelled");
        }

        let result = create_repo(
            &client,
            &host,
            &RepoCreateInput {
                name: repo_name.clone(),
                visibility: visibility.clone(),
                owner_login: owner.clone(),
                team_slug: String::new(),
                description,
                homepage_url: String::new(),
                has_issues_enabled: true,
                has_wiki_enabled: true,
                gitignore_template: String::new(),
                license_template: String::new(),
                include_all_branches: false,
                init_readme: false,
                template_repository_id: template_id,
            },
        )
        .await?;

        let result_full_name = result
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(&repo_name);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        ios_eprintln!(
            ios,
            "{} Created repository {} on {}",
            cs.success_icon(),
            result_full_name,
            host,
        );
        ios_eprintln!(ios, "  {html_url}");

        let should_clone = prompter.confirm("Clone the new repository locally?", true)?;
        if should_clone {
            let protocol = {
                let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
                cfg.git_protocol(&host)
            };
            let clone_owner = result
                .pointer("/owner/login")
                .and_then(Value::as_str)
                .unwrap_or(&owner);
            let clone_name = result
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&repo_name);
            let repo_ref = Repo::with_host(clone_owner, clone_name, &host);
            let remote_url = url_parser::clone_url(&repo_ref, &protocol);
            clone_with_retry(&remote_url, &template_main_branch).await?;
        }

        Ok(())
    }

    /// Create a remote repo from an existing local repo.
    #[allow(clippy::too_many_lines)]
    async fn create_from_local(&self, factory: &Factory, interactive: bool) -> Result<()> {
        let cfg_lock = factory.config()?;
        let host = {
            let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            cfg.authentication()
                .hosts()
                .into_iter()
                .next()
                .unwrap_or_else(|| "github.com".to_string())
        };
        let client = factory.api_client(&host)?;
        let ios = &factory.io;
        let cs = ios.color_scheme();
        let prompter = factory.prompter();

        let source_path = if interactive {
            prompter.input("Path to local repository", ".")?
        } else {
            self.source.clone().unwrap_or_else(|| ".".to_string())
        };

        let abs_path = std::path::absolute(Path::new(&source_path))
            .context("failed to resolve absolute path")?;
        let abs_path_str = abs_path.display().to_string();

        let repo_type = local_repo_type(&source_path).await?;
        match repo_type {
            LocalRepoType::Unknown => {
                if source_path == "." {
                    bail!(
                        "current directory is not a git repository. Run `git init` to initialize it"
                    );
                }
                bail!(
                    "{abs_path_str} is not a git repository. Run `git -C \"{source_path}\" init` to initialize it",
                );
            }
            LocalRepoType::Working | LocalRepoType::Bare => {}
        }

        let has_commits = check_has_commits(&source_path).await?;
        if self.push && !has_commits {
            bail!("`--push` enabled but no commits found in {abs_path_str}");
        }

        let base_remote = self.remote.as_deref().unwrap_or("origin").to_string();

        let (name, description, visibility) = if interactive {
            let dir_name = abs_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            interactive_repo_info(&client, &host, prompter.as_ref(), &dir_name).await?
        } else {
            let name = self.name.clone().unwrap_or_else(|| {
                abs_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            let vis = if self.public {
                "PUBLIC".to_string()
            } else if self.internal {
                "INTERNAL".to_string()
            } else {
                "PRIVATE".to_string()
            };
            (name, self.description.clone().unwrap_or_default(), vis)
        };

        // Parse owner/name
        let (owner, repo_name) = if name.contains('/') {
            let repo =
                Repo::from_full_name(&name).map_err(|e| anyhow::anyhow!("argument error: {e}"))?;
            (repo.owner().to_string(), repo.name().to_string())
        } else {
            (String::new(), name.clone())
        };

        let result = create_repo(
            &client,
            &host,
            &RepoCreateInput {
                name: repo_name.clone(),
                visibility,
                owner_login: owner.clone(),
                team_slug: self.team.clone().unwrap_or_default(),
                description,
                homepage_url: self.homepage.clone().unwrap_or_default(),
                has_issues_enabled: !self.disable_issues,
                has_wiki_enabled: !self.disable_wiki,
                gitignore_template: String::new(),
                license_template: String::new(),
                include_all_branches: false,
                init_readme: false,
                template_repository_id: String::new(),
            },
        )
        .await?;

        let result_full_name = result
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(&repo_name);
        let html_url = result.get("html_url").and_then(Value::as_str).unwrap_or("");

        if ios.is_stdout_tty() {
            ios_eprintln!(
                ios,
                "{} Created repository {} on {}",
                cs.success_icon(),
                result_full_name,
                host,
            );
            ios_eprintln!(ios, "  {html_url}");
        } else {
            ios_println!(ios, "{html_url}");
        }

        let protocol = {
            let cfg = cfg_lock.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            cfg.git_protocol(&host)
        };
        let clone_owner = result
            .pointer("/owner/login")
            .and_then(Value::as_str)
            .unwrap_or(&owner);
        let clone_name = result
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&repo_name);
        let repo_ref = Repo::with_host(clone_owner, clone_name, &host);
        let remote_url = url_parser::clone_url(&repo_ref, &protocol);

        let mut actual_remote = base_remote.clone();

        if interactive {
            let add_remote = prompter.confirm("Add a remote?", true)?;
            if !add_remote {
                return Ok(());
            }
            actual_remote = prompter.input("What should the new remote be called?", "origin")?;
        }

        source_add_remote(ios, &cs, &source_path, &actual_remote, &remote_url).await?;

        // Handle push
        let should_push = if interactive && has_commits {
            let msg = if repo_type == LocalRepoType::Bare {
                format!("Would you like to mirror all refs to \"{actual_remote}\"?")
            } else {
                format!(
                    "Would you like to push commits from the current branch to \"{actual_remote}\"?",
                )
            };
            prompter.confirm(&msg, true)?
        } else {
            self.push
        };

        if should_push && repo_type == LocalRepoType::Working {
            let status = tokio::process::Command::new("git")
                .args([
                    "-C",
                    &source_path,
                    "push",
                    "--set-upstream",
                    &actual_remote,
                    "HEAD",
                ])
                .status()
                .await
                .context("failed to push to remote")?;
            if !status.success() {
                bail!("git push failed");
            }
            if ios.is_stdout_tty() {
                ios_eprintln!(
                    ios,
                    "{} Pushed commits to {}",
                    cs.success_icon(),
                    remote_url,
                );
            }
        }

        if should_push && repo_type == LocalRepoType::Bare {
            let status = tokio::process::Command::new("git")
                .args(["-C", &source_path, "push", &actual_remote, "--mirror"])
                .status()
                .await
                .context("failed to mirror to remote")?;
            if !status.success() {
                bail!("git push --mirror failed");
            }
            if ios.is_stdout_tty() {
                ios_eprintln!(
                    ios,
                    "{} Mirrored all refs to {}",
                    cs.success_icon(),
                    remote_url,
                );
            }
        }

        Ok(())
    }
}

/// Input for creating a repository.
#[allow(clippy::struct_excessive_bools)]
struct RepoCreateInput {
    name: String,
    visibility: String,
    owner_login: String,
    team_slug: String,
    description: String,
    homepage_url: String,
    has_issues_enabled: bool,
    has_wiki_enabled: bool,
    gitignore_template: String,
    license_template: String,
    include_all_branches: bool,
    init_readme: bool,
    template_repository_id: String,
}

/// Create a repository via the GitHub API.
///
/// Uses the GraphQL API for template repos and plain repos without file init.
/// Uses the REST API when gitignore, license, or README init is needed.
async fn create_repo(
    client: &ghc_api::client::Client,
    hostname: &str,
    input: &RepoCreateInput,
) -> Result<Value> {
    let is_org = !input.owner_login.is_empty();
    let is_internal = input.visibility.eq_ignore_ascii_case("internal");

    if is_internal && !is_org {
        bail!("internal repositories can only be created within an organization");
    }

    // Template repository flow uses GraphQL
    if !input.template_repository_id.is_empty() {
        return create_from_template_api(client, hostname, input).await;
    }

    // If we need gitignore, license, or README, use REST API (v3)
    if !input.gitignore_template.is_empty()
        || !input.license_template.is_empty()
        || input.init_readme
    {
        return create_with_rest_api(client, input, is_org).await;
    }

    // Otherwise use GraphQL for a clean create
    create_with_graphql(client, hostname, input).await
}

/// Create a repo from a template using GraphQL.
async fn create_from_template_api(
    client: &ghc_api::client::Client,
    _hostname: &str,
    input: &RepoCreateInput,
) -> Result<Value> {
    // Get owner ID
    let owner_id = if input.owner_login.is_empty() {
        // Get current user ID
        let viewer: Value = client
            .graphql("query { viewer { id } }", &HashMap::new())
            .await
            .context("failed to get current user")?;
        viewer
            .pointer("/viewer/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        let owner_resp: Value = client
            .rest(
                reqwest::Method::GET,
                &format!("users/{}", input.owner_login),
                None,
            )
            .await
            .context("failed to resolve repository owner")?;
        owner_resp
            .get("node_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    let mut variables = HashMap::new();
    variables.insert(
        "input".to_string(),
        serde_json::json!({
            "name": input.name,
            "description": input.description,
            "visibility": input.visibility.to_uppercase(),
            "ownerId": owner_id,
            "repositoryId": input.template_repository_id,
            "includeAllBranches": input.include_all_branches,
        }),
    );

    let mutation = r"
        mutation CloneTemplateRepository($input: CloneTemplateRepositoryInput!) {
            cloneTemplateRepository(input: $input) {
                repository {
                    id
                    name
                    owner { login }
                    url
                }
            }
        }
    ";

    let result: Value = client
        .graphql(mutation, &variables)
        .await
        .context("failed to create repository from template")?;

    let repo = result
        .pointer("/cloneTemplateRepository/repository")
        .cloned()
        .unwrap_or_default();

    // Convert GraphQL response to a common format
    let owner_login = repo
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let repo_name = repo.get("name").and_then(Value::as_str).unwrap_or_default();
    let url = repo.get("url").and_then(Value::as_str).unwrap_or_default();

    Ok(serde_json::json!({
        "full_name": format!("{owner_login}/{repo_name}"),
        "name": repo_name,
        "html_url": url,
        "owner": { "login": owner_login },
    }))
}

/// Create a repository using the REST v3 API (needed for gitignore/license/readme init).
async fn create_with_rest_api(
    client: &ghc_api::client::Client,
    input: &RepoCreateInput,
    is_org: bool,
) -> Result<Value> {
    let mut body = serde_json::json!({
        "name": input.name,
        "private": input.visibility.eq_ignore_ascii_case("private"),
        "has_issues": input.has_issues_enabled,
        "has_wiki": input.has_wiki_enabled,
        "auto_init": input.init_readme,
    });

    if !input.description.is_empty() {
        body["description"] = Value::String(input.description.clone());
    }
    if !input.homepage_url.is_empty() {
        body["homepage"] = Value::String(input.homepage_url.clone());
    }
    if !input.gitignore_template.is_empty() {
        body["gitignore_template"] = Value::String(input.gitignore_template.clone());
    }
    if !input.license_template.is_empty() {
        body["license_template"] = Value::String(input.license_template.clone());
    }
    if is_org && input.visibility.eq_ignore_ascii_case("internal") {
        body["visibility"] = Value::String("internal".to_string());
    }

    let path = if is_org {
        format!("orgs/{}/repos", input.owner_login)
    } else {
        "user/repos".to_string()
    };

    let result: Value = client
        .rest(reqwest::Method::POST, &path, Some(&body))
        .await
        .context("failed to create repository")?;

    Ok(result)
}

/// Create a repository using the GraphQL API (no file initialization).
async fn create_with_graphql(
    client: &ghc_api::client::Client,
    _hostname: &str,
    input: &RepoCreateInput,
) -> Result<Value> {
    // Resolve owner ID if needed
    let owner_id = if input.owner_login.is_empty() {
        String::new()
    } else {
        let owner_resp: Value = client
            .rest(
                reqwest::Method::GET,
                &format!("users/{}", input.owner_login),
                None,
            )
            .await
            .context("failed to resolve repository owner")?;
        owner_resp
            .get("node_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };

    let team_id = if !input.team_slug.is_empty() && !input.owner_login.is_empty() {
        let team_resp: Value = client
            .rest(
                reqwest::Method::GET,
                &format!("orgs/{}/teams/{}", input.owner_login, input.team_slug),
                None,
            )
            .await
            .context("failed to resolve organization team")?;
        team_resp
            .get("node_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        String::new()
    };

    let mut gql_input = serde_json::json!({
        "name": input.name,
        "description": input.description,
        "homepageUrl": input.homepage_url,
        "visibility": input.visibility.to_uppercase(),
        "hasIssuesEnabled": input.has_issues_enabled,
        "hasWikiEnabled": input.has_wiki_enabled,
    });

    if !owner_id.is_empty() {
        gql_input["ownerId"] = Value::String(owner_id);
    }
    if !team_id.is_empty() {
        gql_input["teamId"] = Value::String(team_id);
    }

    let mut variables = HashMap::new();
    variables.insert("input".to_string(), gql_input);

    let mutation = r"
        mutation RepositoryCreate($input: CreateRepositoryInput!) {
            createRepository(input: $input) {
                repository {
                    id
                    name
                    owner { login }
                    url
                }
            }
        }
    ";

    let result: Value = client
        .graphql(mutation, &variables)
        .await
        .context("failed to create repository")?;

    let repo = result
        .pointer("/createRepository/repository")
        .cloned()
        .unwrap_or_default();

    let owner_login = repo
        .pointer("/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let repo_name = repo.get("name").and_then(Value::as_str).unwrap_or_default();
    let url = repo.get("url").and_then(Value::as_str).unwrap_or_default();

    Ok(serde_json::json!({
        "full_name": format!("{owner_login}/{repo_name}"),
        "name": repo_name,
        "html_url": url,
        "owner": { "login": owner_login },
    }))
}

/// Resolve a template repository name to its node ID and default branch.
async fn resolve_template_repo(
    client: &ghc_api::client::Client,
    host: &str,
    template_name: &str,
) -> Result<(String, String)> {
    let full_name = if template_name.contains('/') {
        template_name.to_string()
    } else {
        let username = client
            .current_login()
            .await
            .context("failed to get current user")?;
        format!("{username}/{template_name}")
    };

    let repo =
        Repo::from_full_name(&full_name).map_err(|e| anyhow::anyhow!("argument error: {e}"))?;

    let mut variables = HashMap::new();
    variables.insert("owner".to_string(), Value::String(repo.owner().to_string()));
    variables.insert("name".to_string(), Value::String(repo.name().to_string()));

    let query = r"
        query RepositoryInfo($owner: String!, $name: String!) {
            repository(owner: $owner, name: $name) {
                id
                defaultBranchRef { name }
            }
        }
    ";

    let result: Value = client
        .graphql(query, &variables)
        .await
        .with_context(|| format!("failed to fetch template repository {full_name} on {host}"))?;

    let id = result
        .pointer("/repository/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let branch = result
        .pointer("/repository/defaultBranchRef/name")
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_string();

    Ok((id, branch))
}

/// Interactive prompt for repository name, owner, description, and visibility.
async fn interactive_repo_info(
    client: &ghc_api::client::Client,
    host: &str,
    prompter: &dyn Prompter,
    default_name: &str,
) -> Result<(String, String, String)> {
    let (name, owner) =
        interactive_repo_name_and_owner(client, host, prompter, default_name).await?;

    let full_name = if owner.is_empty() {
        name.clone()
    } else {
        format!("{owner}/{name}")
    };

    let description = prompter.input("Description", "")?;

    let visibility_options = if owner.is_empty() {
        vec!["Public".to_string(), "Private".to_string()]
    } else {
        vec![
            "Public".to_string(),
            "Private".to_string(),
            "Internal".to_string(),
        ]
    };

    let selected = prompter.select("Visibility", Some(0), &visibility_options)?;
    let visibility = visibility_options[selected].to_uppercase();

    Ok((full_name, description, visibility))
}

/// Interactive prompt for repository name and owner.
async fn interactive_repo_name_and_owner(
    client: &ghc_api::client::Client,
    _host: &str,
    prompter: &dyn Prompter,
    default_name: &str,
) -> Result<(String, String)> {
    let raw_name = prompter.input("Repository name", default_name)?;

    // Check if user provided owner/name format
    if raw_name.contains('/') {
        let repo =
            Repo::from_full_name(&raw_name).map_err(|e| anyhow::anyhow!("argument error: {e}"))?;
        return Ok((repo.name().to_string(), repo.owner().to_string()));
    }

    // Get user and orgs
    let username = client
        .current_login()
        .await
        .context("failed to get current user")?;

    let orgs = get_user_orgs(client).await.unwrap_or_default();
    if orgs.is_empty() {
        return Ok((raw_name, String::new()));
    }

    let mut owners: Vec<String> = orgs;
    owners.push(username.clone());
    owners.sort();

    let default_idx = owners.iter().position(|o| o == &username);
    let selected = prompter.select("Repository owner", default_idx, &owners)?;

    let owner = &owners[selected];
    if owner == &username {
        Ok((raw_name, String::new()))
    } else {
        Ok((raw_name, owner.clone()))
    }
}

/// Get the organizations the current user belongs to.
async fn get_user_orgs(client: &ghc_api::client::Client) -> Result<Vec<String>> {
    let result: Value = client
        .graphql(
            r"query { viewer { organizations(first: 100) { nodes { login } } } }",
            &HashMap::new(),
        )
        .await
        .context("failed to list organizations")?;

    let orgs = result
        .pointer("/viewer/organizations/nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|n| n.get("login").and_then(Value::as_str).map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(orgs)
}

/// Interactive prompt for selecting a template repository.
async fn interactive_repo_template(
    client: &ghc_api::client::Client,
    _host: &str,
    owner: &str,
    prompter: &dyn Prompter,
) -> Result<Value> {
    let mut variables = HashMap::new();
    variables.insert("owner".to_string(), Value::String(owner.to_string()));
    variables.insert("perPage".to_string(), serde_json::json!(100));

    let query = r"
        query RepositoryList($owner: String!, $perPage: Int!) {
            repositoryOwner(login: $owner) {
                repositories(first: $perPage, ownerAffiliations: OWNER, orderBy: {field: PUSHED_AT, direction: DESC}) {
                    nodes {
                        id
                        name
                        isTemplate
                        defaultBranchRef { name }
                    }
                }
            }
        }
    ";

    let result: Value = client
        .graphql(query, &variables)
        .await
        .context("failed to list template repositories")?;

    let repos: Vec<Value> = result
        .pointer("/repositoryOwner/repositories/nodes")
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter(|r| {
                    r.get("isTemplate")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    if repos.is_empty() {
        bail!("{owner} has no template repositories");
    }

    let names: Vec<String> = repos
        .iter()
        .filter_map(|r| r.get("name").and_then(Value::as_str).map(String::from))
        .collect();

    let selected = prompter.select("Choose a template repository", None, &names)?;
    Ok(repos[selected].clone())
}

/// Interactive prompt for selecting a gitignore template.
async fn interactive_gitignore(
    client: &ghc_api::client::Client,
    _host: &str,
    prompter: &dyn Prompter,
) -> Result<String> {
    let confirmed = prompter.confirm("Would you like to add a .gitignore?", false)?;
    if !confirmed {
        return Ok(String::new());
    }

    let templates: Vec<String> = client
        .rest(reqwest::Method::GET, "gitignore/templates", None)
        .await
        .context("failed to fetch gitignore templates")?;

    let selected = prompter.select("Choose a .gitignore template", None, &templates)?;
    Ok(templates[selected].clone())
}

/// Interactive prompt for selecting a license template.
async fn interactive_license(
    client: &ghc_api::client::Client,
    _host: &str,
    prompter: &dyn Prompter,
) -> Result<String> {
    let confirmed = prompter.confirm("Would you like to add a license?", false)?;
    if !confirmed {
        return Ok(String::new());
    }

    let licenses: Vec<Value> = client
        .rest(reqwest::Method::GET, "licenses", None)
        .await
        .context("failed to fetch licenses")?;

    let names: Vec<String> = licenses
        .iter()
        .filter_map(|l| l.get("name").and_then(Value::as_str).map(String::from))
        .collect();

    let selected = prompter.select("Choose a license", None, &names)?;

    let key = licenses[selected]
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(key)
}

/// Regex for characters not allowed in repository names.
static REPO_NAME_INVALID_CHARS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\w._-]+").expect("REPO_NAME_INVALID_CHARS is a valid regex"));

/// Normalize a repository name (replace invalid characters with hyphens).
fn normalize_repo_name(name: &str) -> String {
    let result = REPO_NAME_INVALID_CHARS.replace_all(name, "-");
    result.trim_end_matches(".git").to_string()
}

/// Initialize a local directory with git and set up a remote.
async fn local_init(remote_url: &str, path: &str) -> Result<()> {
    let status = tokio::process::Command::new("git")
        .args(["init", path])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .context("failed to run git init")?;
    if !status.success() {
        bail!("git init failed");
    }

    let status = tokio::process::Command::new("git")
        .args(["-C", path, "remote", "add", "origin", remote_url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .context("failed to add remote")?;
    if !status.success() {
        bail!("failed to add remote origin");
    }

    Ok(())
}

/// Clone a repository with retry logic for template repos that may not be ready.
async fn clone_with_retry(remote_url: &str, branch: &str) -> Result<()> {
    let mut last_err = None;

    for attempt in 0..=CLONE_MAX_RETRIES {
        let mut args = vec!["clone".to_string()];
        if !branch.is_empty() {
            args.push("--branch".to_string());
            args.push(branch.to_string());
        }
        args.push(remote_url.to_string());

        let output = tokio::process::Command::new("git")
            .args(&args)
            .output()
            .await
            .context("failed to run git clone")?;

        if output.status.success() {
            return Ok(());
        }

        let exit_code = output.status.code().unwrap_or(1);
        if exit_code == 128 && attempt < CLONE_MAX_RETRIES {
            // Retryable error (repo might not be ready yet)
            tokio::time::sleep(CLONE_RETRY_DELAY).await;
            last_err = Some(String::from_utf8_lossy(&output.stderr).to_string());
            continue;
        }

        // Non-retryable or exhausted retries
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {stderr}");
    }

    bail!(
        "git clone failed after {} retries: {}",
        CLONE_MAX_RETRIES,
        last_err.unwrap_or_default(),
    );
}

/// Add a remote to a local repository.
async fn source_add_remote(
    ios: &IOStreams,
    cs: &ghc_core::iostreams::ColorScheme,
    source_path: &str,
    remote_name: &str,
    remote_url: &str,
) -> Result<()> {
    let status = tokio::process::Command::new("git")
        .args(["-C", source_path, "remote", "add", remote_name, remote_url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .context("failed to add remote")?;

    if !status.success() {
        bail!("Unable to add remote \"{remote_name}\"");
    }

    if ios.is_stdout_tty() {
        ios_eprintln!(ios, "{} Added remote {}", cs.success_icon(), remote_url);
    }

    Ok(())
}

/// Detect the type of local repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalRepoType {
    Unknown,
    Working,
    Bare,
}

async fn local_repo_type(source_path: &str) -> Result<LocalRepoType> {
    let output = tokio::process::Command::new("git")
        .args(["-C", source_path, "rev-parse", "--git-dir"])
        .output()
        .await;

    let Ok(output) = output else {
        return Ok(LocalRepoType::Unknown);
    };

    if !output.status.success() {
        return Ok(LocalRepoType::Unknown);
    }

    let git_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match git_dir.as_str() {
        "." => Ok(LocalRepoType::Bare),
        ".git" => Ok(LocalRepoType::Working),
        _ => Ok(LocalRepoType::Unknown),
    }
}

/// Check if a local repository has any commits.
async fn check_has_commits(source_path: &str) -> Result<bool> {
    let output = tokio::process::Command::new("git")
        .args(["-C", source_path, "rev-parse", "HEAD"])
        .output()
        .await
        .context("failed to check for commits")?;

    if output.status.success() {
        return Ok(true);
    }

    let exit_code = output.status.code().unwrap_or(1);
    if exit_code == 128 {
        return Ok(false);
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::{TestHarness, mock_graphql, mock_rest_get, mock_rest_post};

    #[tokio::test]
    async fn test_should_create_repository_with_rest_api() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/user/repos",
            201,
            serde_json::json!({
                "full_name": "testuser/new-repo",
                "name": "new-repo",
                "html_url": "https://github.com/testuser/new-repo",
                "clone_url": "https://github.com/testuser/new-repo.git",
                "owner": { "login": "testuser" },
            }),
        )
        .await;

        let args = CreateArgs {
            name: Some("new-repo".into()),
            description: Some("My new repo".into()),
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: true,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        args.run(&h.factory).await.unwrap();

        // Non-TTY output: URL goes to stdout
        let out = h.stdout();
        assert!(
            out.contains("https://github.com/testuser/new-repo"),
            "expected URL in stdout, got: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_create_repository_with_graphql() {
        let h = TestHarness::new().await;
        mock_graphql(
            &h.server,
            "createRepository",
            serde_json::json!({
                "data": {
                    "createRepository": {
                        "repository": {
                            "id": "R_123",
                            "name": "new-repo",
                            "owner": { "login": "testuser" },
                            "url": "https://github.com/testuser/new-repo",
                        }
                    }
                }
            }),
        )
        .await;

        let args = CreateArgs {
            name: Some("new-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        args.run(&h.factory).await.unwrap();

        // Non-TTY output: URL goes to stdout
        let out = h.stdout();
        assert!(
            out.contains("https://github.com/testuser/new-repo"),
            "expected URL in stdout, got: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_fail_without_name_in_non_interactive() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: None,
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("name argument required")
        );
    }

    #[tokio::test]
    async fn test_should_fail_without_visibility_flags() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: false,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("--public") || msg.contains("--private") || msg.contains("--internal")
        );
    }

    #[tokio::test]
    async fn test_should_reject_remote_without_source() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: Some("upstream".into()),
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--remote"));
    }

    #[tokio::test]
    async fn test_should_reject_push_without_source() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: true,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--push"));
    }

    #[tokio::test]
    async fn test_should_reject_source_with_clone() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: true,
            add_readme: false,
            license: None,
            gitignore: None,
            source: Some(".".into()),
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--source"));
    }

    #[tokio::test]
    async fn test_should_reject_template_with_gitignore() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: Some("tmpl-repo".into()),
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: Some("Rust".into()),
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("gitignore"));
    }

    #[tokio::test]
    async fn test_should_reject_template_with_add_readme() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: Some("tmpl-repo".into()),
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: true,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--add-readme"));
    }

    #[tokio::test]
    async fn test_should_reject_include_all_branches_without_template() {
        let h = TestHarness::new().await;

        let args = CreateArgs {
            name: Some("my-repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: true,
            disable_issues: false,
            disable_wiki: false,
        };
        let result = args.run(&h.factory).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("--include-all-branches")
        );
    }

    #[tokio::test]
    async fn test_should_create_org_repo_with_rest() {
        let h = TestHarness::new().await;
        mock_rest_post(
            &h.server,
            "/orgs/my-org/repos",
            201,
            serde_json::json!({
                "full_name": "my-org/new-repo",
                "name": "new-repo",
                "html_url": "https://github.com/my-org/new-repo",
                "owner": { "login": "my-org" },
            }),
        )
        .await;

        let args = CreateArgs {
            name: Some("my-org/new-repo".into()),
            description: Some("Org repo".into()),
            homepage: None,
            team: None,
            template: None,
            public: false,
            private: true,
            internal: false,
            clone: false,
            add_readme: true,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        args.run(&h.factory).await.unwrap();

        // Non-TTY output: URL goes to stdout
        let out = h.stdout();
        assert!(
            out.contains("https://github.com/my-org/new-repo"),
            "expected URL in stdout, got: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_create_repo_with_template_via_graphql() {
        let h = TestHarness::new().await;

        // Mock current user login
        mock_graphql(
            &h.server,
            "UserCurrent",
            serde_json::json!({
                "data": {
                    "viewer": { "login": "testuser" }
                }
            }),
        )
        .await;

        // Mock template repo lookup
        mock_graphql(
            &h.server,
            "RepositoryInfo",
            serde_json::json!({
                "data": {
                    "repository": {
                        "id": "R_template_123",
                        "defaultBranchRef": { "name": "main" }
                    }
                }
            }),
        )
        .await;

        // Mock viewer ID
        mock_graphql(
            &h.server,
            "viewer",
            serde_json::json!({
                "data": {
                    "viewer": { "id": "U_abc123" }
                }
            }),
        )
        .await;

        // Mock clone template mutation
        mock_graphql(
            &h.server,
            "CloneTemplateRepository",
            serde_json::json!({
                "data": {
                    "cloneTemplateRepository": {
                        "repository": {
                            "id": "R_new_123",
                            "name": "from-template",
                            "owner": { "login": "testuser" },
                            "url": "https://github.com/testuser/from-template",
                        }
                    }
                }
            }),
        )
        .await;

        let args = CreateArgs {
            name: Some("from-template".into()),
            description: None,
            homepage: None,
            team: None,
            template: Some("my-template".into()),
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };
        args.run(&h.factory).await.unwrap();

        // Non-TTY output: URL goes to stdout
        let out = h.stdout();
        assert!(
            out.contains("https://github.com/testuser/from-template"),
            "expected URL in stdout, got: {out}"
        );
    }

    #[tokio::test]
    async fn test_should_create_interactive_from_scratch() {
        let h = TestHarness::new().await;

        // Configure stub prompter answers
        // 1. "What would you like to do?" -> 0 (from scratch)
        // 2. "Repository name" -> "test-repo"
        // 3. current_login GraphQL -> "testuser"
        // 4. orgs GraphQL -> empty
        // 5. "Description" -> "A test repo"
        // 6. "Visibility" -> 0 (Public)
        // 7. "Would you like to add a README file?" -> false
        // 8. "Would you like to add a .gitignore?" -> false
        // 9. "Would you like to add a license?" -> false
        // 10. Confirm -> true
        // 11. "Clone the new repository locally?" -> false
        h.prompter.select_answers.lock().unwrap().extend([0, 0]); // "What would you like to do?", "Visibility"
        h.prompter
            .input_answers
            .lock()
            .unwrap()
            .extend(["test-repo".to_string(), "A test repo".to_string()]);
        h.prompter
            .confirm_answers
            .lock()
            .unwrap()
            .extend([false, false, false, true, false]); // readme, gitignore, license, confirm, clone

        // Make IOStreams interactive
        // (TestHarness creates non-interactive streams by default; we need can_prompt() = true)
        // can_prompt needs stdin_is_tty && stdout_is_tty && !never_prompt
        // But the test factory has never_prompt=true. We can't easily change that.
        // So we'll test the non-interactive path instead, and validate the interactive
        // path indirectly through the unit tests above.

        // For interactive test, we call run_interactive directly
        // Mock GraphQL for current_login
        mock_graphql(
            &h.server,
            "UserCurrent",
            serde_json::json!({
                "data": { "viewer": { "login": "testuser" } }
            }),
        )
        .await;

        // Mock orgs
        mock_graphql(
            &h.server,
            "organizations",
            serde_json::json!({
                "data": {
                    "viewer": {
                        "organizations": {
                            "nodes": []
                        }
                    }
                }
            }),
        )
        .await;

        // Mock gitignore templates
        mock_rest_get(
            &h.server,
            "/gitignore/templates",
            serde_json::json!(["Rust", "Go", "Python"]),
        )
        .await;

        // Mock licenses
        mock_rest_get(
            &h.server,
            "/licenses",
            serde_json::json!([
                {"key": "mit", "name": "MIT License"},
                {"key": "apache-2.0", "name": "Apache License 2.0"},
            ]),
        )
        .await;

        // Mock repo creation via GraphQL
        mock_graphql(
            &h.server,
            "createRepository",
            serde_json::json!({
                "data": {
                    "createRepository": {
                        "repository": {
                            "id": "R_123",
                            "name": "test-repo",
                            "owner": { "login": "testuser" },
                            "url": "https://github.com/testuser/test-repo",
                        }
                    }
                }
            }),
        )
        .await;

        let args = CreateArgs {
            name: None,
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: false,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };

        // Call run_interactive directly since can_prompt() is false
        args.run_interactive(&h.factory).await.unwrap();

        // Non-TTY output: URL goes to stdout
        let out = h.stdout();
        assert!(
            out.contains("https://github.com/testuser/test-repo"),
            "expected URL in stdout, got: {out}"
        );
    }

    #[test]
    fn test_should_normalize_repo_name() {
        assert_eq!(normalize_repo_name("my repo"), "my-repo");
        assert_eq!(normalize_repo_name("my.repo"), "my.repo");
        assert_eq!(normalize_repo_name("my-repo.git"), "my-repo");
        assert_eq!(normalize_repo_name("hello world!"), "hello-world-");
        assert_eq!(normalize_repo_name("valid_name-123"), "valid_name-123");
    }

    #[test]
    fn test_should_validate_source_with_clone_conflict() {
        let args = CreateArgs {
            name: Some("repo".into()),
            description: None,
            homepage: None,
            team: None,
            template: None,
            public: true,
            private: false,
            internal: false,
            clone: true,
            add_readme: false,
            license: None,
            gitignore: None,
            source: Some(".".into()),
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };

        // Use a minimal Factory for validation
        let (factory, _) = Factory::test();
        let result = args.validate_flags(&factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--source"));
    }

    #[test]
    fn test_should_validate_template_with_team_conflict() {
        let args = CreateArgs {
            name: Some("repo".into()),
            description: None,
            homepage: None,
            team: Some("eng".into()),
            template: Some("tmpl".into()),
            public: true,
            private: false,
            internal: false,
            clone: false,
            add_readme: false,
            license: None,
            gitignore: None,
            source: None,
            remote: None,
            push: false,
            include_all_branches: false,
            disable_issues: false,
            disable_wiki: false,
        };

        let (factory, _) = Factory::test();
        let result = args.validate_flags(&factory);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--template"));
    }
}
