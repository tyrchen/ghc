//! Interactive prompt handling.
//!
//! Maps from Go's `internal/prompter` package.

use anyhow::Result;

/// Trait for interactive terminal prompts.
pub trait Prompter: Send + Sync + std::fmt::Debug {
    /// Present a list of options and return the selected index.
    fn select(&self, prompt: &str, default: Option<usize>, options: &[String]) -> Result<usize>;

    /// Present a list of options for multiple selection.
    fn multi_select(
        &self,
        prompt: &str,
        defaults: &[bool],
        options: &[String],
    ) -> Result<Vec<usize>>;

    /// Prompt for free-text input.
    fn input(&self, prompt: &str, default: &str) -> Result<String>;

    /// Prompt for password input (hidden).
    fn password(&self, prompt: &str) -> Result<String>;

    /// Prompt for yes/no confirmation.
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;

    /// Open an editor for multi-line text input.
    fn editor(&self, prompt: &str, default: &str, allow_blank: bool) -> Result<String>;
}

/// Dialoguer-based prompter implementation.
#[derive(Debug)]
pub struct DialoguerPrompter {
    editor_cmd: Option<String>,
}

impl DialoguerPrompter {
    /// Create a new prompter.
    pub fn new(editor_cmd: Option<String>) -> Self {
        Self { editor_cmd }
    }
}

impl Prompter for DialoguerPrompter {
    fn select(&self, prompt: &str, default: Option<usize>, options: &[String]) -> Result<usize> {
        let mut sel = dialoguer::Select::new().with_prompt(prompt).items(options);
        if let Some(d) = default {
            sel = sel.default(d);
        }
        Ok(sel.interact()?)
    }

    fn multi_select(
        &self,
        prompt: &str,
        defaults: &[bool],
        options: &[String],
    ) -> Result<Vec<usize>> {
        let sel = dialoguer::MultiSelect::new()
            .with_prompt(prompt)
            .items(options)
            .defaults(defaults);
        Ok(sel.interact()?)
    }

    fn input(&self, prompt: &str, default: &str) -> Result<String> {
        let mut input = dialoguer::Input::new().with_prompt(prompt);
        if !default.is_empty() {
            input = input.default(default.to_string());
        }
        Ok(input.interact_text()?)
    }

    fn password(&self, prompt: &str) -> Result<String> {
        Ok(dialoguer::Password::new().with_prompt(prompt).interact()?)
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool> {
        Ok(dialoguer::Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()?)
    }

    fn editor(&self, _prompt: &str, default: &str, _allow_blank: bool) -> Result<String> {
        let editor = self
            .editor_cmd
            .as_deref()
            .or_else(|| std::env::var("EDITOR").ok().as_deref().map(|_| ""))
            .unwrap_or("vi");

        // Use dialoguer's editor
        let mut e = dialoguer::Editor::new();
        if !editor.is_empty() {
            e.executable(editor);
        }

        let result = e.edit(default)?;
        Ok(result.unwrap_or_default())
    }
}

/// Stub prompter for testing that returns pre-configured answers.
#[derive(Debug, Default)]
pub struct StubPrompter {
    /// Pre-configured select answers (index).
    pub select_answers: std::sync::Mutex<Vec<usize>>,
    /// Pre-configured input answers.
    pub input_answers: std::sync::Mutex<Vec<String>>,
    /// Pre-configured confirm answers.
    pub confirm_answers: std::sync::Mutex<Vec<bool>>,
}

impl Prompter for StubPrompter {
    fn select(&self, _prompt: &str, default: Option<usize>, _options: &[String]) -> Result<usize> {
        let mut answers = self
            .select_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if answers.is_empty() {
            Ok(default.unwrap_or(0))
        } else {
            Ok(answers.remove(0))
        }
    }

    fn multi_select(
        &self,
        _prompt: &str,
        _defaults: &[bool],
        _options: &[String],
    ) -> Result<Vec<usize>> {
        Ok(vec![])
    }

    fn input(&self, _prompt: &str, default: &str) -> Result<String> {
        let mut answers = self
            .input_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if answers.is_empty() {
            Ok(default.to_string())
        } else {
            Ok(answers.remove(0))
        }
    }

    fn password(&self, _prompt: &str) -> Result<String> {
        let mut answers = self
            .input_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if answers.is_empty() {
            Ok(String::new())
        } else {
            Ok(answers.remove(0))
        }
    }

    fn confirm(&self, _prompt: &str, default: bool) -> Result<bool> {
        let mut answers = self
            .confirm_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if answers.is_empty() {
            Ok(default)
        } else {
            Ok(answers.remove(0))
        }
    }

    fn editor(&self, _prompt: &str, default: &str, _allow_blank: bool) -> Result<String> {
        let mut answers = self
            .input_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if answers.is_empty() {
            Ok(default.to_string())
        } else {
            Ok(answers.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_return_default_select_when_empty() {
        let stub = StubPrompter::default();
        let result = stub
            .select("pick one", Some(2), &["a".into(), "b".into(), "c".into()])
            .unwrap();
        assert_eq!(result, 2);
    }

    #[test]
    fn test_should_return_zero_when_no_default_and_empty() {
        let stub = StubPrompter::default();
        let result = stub
            .select("pick one", None, &["a".into(), "b".into()])
            .unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_should_return_preconfigured_select_answers() {
        let stub = StubPrompter::default();
        stub.select_answers.lock().unwrap().extend([1, 2, 0]);

        assert_eq!(
            stub.select("q1", None, &["a".into(), "b".into(), "c".into()])
                .unwrap(),
            1,
        );
        assert_eq!(
            stub.select("q2", None, &["a".into(), "b".into(), "c".into()])
                .unwrap(),
            2,
        );
        assert_eq!(
            stub.select("q3", None, &["a".into(), "b".into(), "c".into()])
                .unwrap(),
            0,
        );
    }

    #[test]
    fn test_should_return_default_input_when_empty() {
        let stub = StubPrompter::default();
        let result = stub.input("name?", "defaultval").unwrap();
        assert_eq!(result, "defaultval");
    }

    #[test]
    fn test_should_return_preconfigured_input_answers() {
        let stub = StubPrompter::default();
        stub.input_answers
            .lock()
            .unwrap()
            .push("typed value".to_string());

        let result = stub.input("name?", "default").unwrap();
        assert_eq!(result, "typed value");
    }

    #[test]
    fn test_should_return_default_confirm() {
        let stub = StubPrompter::default();
        assert!(stub.confirm("sure?", true).unwrap());
        assert!(!stub.confirm("sure?", false).unwrap());
    }

    #[test]
    fn test_should_return_preconfigured_confirm_answers() {
        let stub = StubPrompter::default();
        stub.confirm_answers.lock().unwrap().push(false);

        assert!(!stub.confirm("sure?", true).unwrap());
    }

    #[test]
    fn test_should_return_empty_password_when_no_answers() {
        let stub = StubPrompter::default();
        let result = stub.password("token?").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_should_return_preconfigured_password() {
        let stub = StubPrompter::default();
        stub.input_answers
            .lock()
            .unwrap()
            .push("secret123".to_string());

        let result = stub.password("token?").unwrap();
        assert_eq!(result, "secret123");
    }

    #[test]
    fn test_should_return_default_editor_when_no_answers() {
        let stub = StubPrompter::default();
        let result = stub.editor("body?", "initial content", true).unwrap();
        assert_eq!(result, "initial content");
    }

    #[test]
    fn test_should_return_preconfigured_editor_answer() {
        let stub = StubPrompter::default();
        stub.input_answers
            .lock()
            .unwrap()
            .push("edited content".to_string());

        let result = stub.editor("body?", "initial", true).unwrap();
        assert_eq!(result, "edited content");
    }

    #[test]
    fn test_should_return_empty_multi_select_by_default() {
        let stub = StubPrompter::default();
        let result = stub
            .multi_select("pick", &[false, false], &["a".into(), "b".into()])
            .unwrap();
        assert!(result.is_empty());
    }
}
