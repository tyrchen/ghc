//! Shared test utilities for the GHC crates.
//!
//! This module is only compiled in test builds (`#[cfg(test)]`).

/// RAII guard for environment variables in tests.
///
/// Sets an environment variable on creation and restores the original value
/// (or removes the variable) when dropped.
#[derive(Debug)]
pub struct EnvVarGuard {
    key: String,
    original: Option<String>,
}

impl EnvVarGuard {
    /// Set an environment variable, returning a guard that restores it on drop.
    pub fn set(key: &str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: Tests are run single-threaded with --test-threads=1
        // when env vars are involved, avoiding data races.
        unsafe { std::env::set_var(key, value) };
        Self {
            key: key.to_string(),
            original,
        }
    }

    /// Remove an environment variable, returning a guard that restores it on drop.
    pub fn unset(key: &str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: Tests are run single-threaded with --test-threads=1
        // when env vars are involved, avoiding data races.
        unsafe { std::env::remove_var(key) };
        Self {
            key: key.to_string(),
            original,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            // SAFETY: See EnvVarGuard::set / EnvVarGuard::unset
            Some(val) => unsafe { std::env::set_var(&self.key, val) },
            None => unsafe { std::env::remove_var(&self.key) },
        }
    }
}
