//! Shell completion command (`ghc completion`).
//!
//! Generate shell completion scripts.

use anyhow::Result;
use clap::Args;

use ghc_core::ios_println;

/// Generate shell completion scripts.
#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// Shell to generate completions for.
    #[arg(value_name = "SHELL", value_parser = ["bash", "zsh", "fish", "powershell"])]
    shell: String,
}

impl CompletionArgs {
    /// Run the completion command.
    ///
    /// # Errors
    ///
    /// Returns an error if the shell is not supported.
    #[allow(clippy::unused_async)]
    pub async fn run(&self, factory: &crate::factory::Factory) -> Result<()> {
        let ios = &factory.io;
        // Note: In a full implementation, we'd use clap_complete to generate
        // the completions from our CLI definition. For now, we output
        // a stub script that delegates to the binary.
        match self.shell.as_str() {
            "bash" => {
                ios_println!(
                    ios,
                    r#"# bash completion for ghc
_ghc_completions()
{{
    local cur prev
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    COMPREPLY=( $(compgen -W "issue pr repo auth gist release run workflow cache search secret variable label config alias ssh-key gpg-key org codespace project ruleset browse completion status api extension actions version" -- "$cur") )
}}
complete -F _ghc_completions ghc"#
                );
            }
            "zsh" => {
                ios_println!(
                    ios,
                    r"#compdef ghc

_ghc() {{
    local -a commands
    commands=(
        'issue:Manage issues'
        'pr:Manage pull requests'
        'repo:Manage repositories'
        'auth:Authenticate with GitHub'
        'gist:Manage gists'
        'release:Manage releases'
        'run:View workflow runs'
        'workflow:Manage workflows'
        'cache:Manage caches'
        'search:Search GitHub'
        'secret:Manage secrets'
        'variable:Manage variables'
        'label:Manage labels'
        'config:Manage configuration'
        'alias:Manage aliases'
        'ssh-key:Manage SSH keys'
        'gpg-key:Manage GPG keys'
        'org:Manage organizations'
        'codespace:Manage codespaces'
        'project:Manage projects'
        'ruleset:Manage rulesets'
        'browse:Open in browser'
        'completion:Generate completions'
        'status:Show status'
        'api:Make API requests'
        'extension:Manage extensions'
        'actions:Learn about Actions'
        'version:Show version'
    )
    _describe 'command' commands
}}

_ghc"
                );
            }
            "fish" => {
                ios_println!(
                    ios,
                    r"# fish completions for ghc
complete -c ghc -n '__fish_use_subcommand' -a issue -d 'Manage issues'
complete -c ghc -n '__fish_use_subcommand' -a pr -d 'Manage pull requests'
complete -c ghc -n '__fish_use_subcommand' -a repo -d 'Manage repositories'
complete -c ghc -n '__fish_use_subcommand' -a auth -d 'Authenticate with GitHub'
complete -c ghc -n '__fish_use_subcommand' -a gist -d 'Manage gists'
complete -c ghc -n '__fish_use_subcommand' -a release -d 'Manage releases'
complete -c ghc -n '__fish_use_subcommand' -a run -d 'View workflow runs'
complete -c ghc -n '__fish_use_subcommand' -a workflow -d 'Manage workflows'
complete -c ghc -n '__fish_use_subcommand' -a cache -d 'Manage caches'
complete -c ghc -n '__fish_use_subcommand' -a search -d 'Search GitHub'
complete -c ghc -n '__fish_use_subcommand' -a version -d 'Show version'"
                );
            }
            "powershell" => {
                ios_println!(
                    ios,
                    r#"Register-ArgumentCompleter -Native -CommandName ghc -ScriptBlock {{
    param($wordToComplete, $commandAst, $cursorPosition)
    $commands = @('issue', 'pr', 'repo', 'auth', 'gist', 'release', 'run', 'workflow', 'cache', 'search', 'secret', 'variable', 'label', 'config', 'alias', 'ssh-key', 'gpg-key', 'org', 'codespace', 'project', 'ruleset', 'browse', 'completion', 'status', 'api', 'extension', 'actions', 'version')
    $commands | Where-Object {{ $_ -like "$wordToComplete*" }} | ForEach-Object {{
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
    }}
}}"#
                );
            }
            other => anyhow::bail!("unsupported shell: {other}"),
        }

        Ok(())
    }
}
