//! Browser integration for opening URLs.
//!
//! Maps from Go's `internal/browser` package.

/// Trait for opening URLs in a browser.
pub trait Browser: Send + Sync + std::fmt::Debug {
    /// Open a URL in the user's browser.
    ///
    /// # Errors
    ///
    /// Returns an error if the browser cannot be opened.
    fn open(&self, url: &str) -> anyhow::Result<()>;
}

/// System browser implementation using the `open` crate.
#[derive(Debug, Clone)]
pub struct SystemBrowser {
    launcher: Option<String>,
}

impl SystemBrowser {
    /// Create a browser that uses the system default.
    pub fn new() -> Self {
        Self { launcher: None }
    }

    /// Create a browser with a specific launcher command.
    pub fn with_launcher(launcher: impl Into<String>) -> Self {
        Self {
            launcher: Some(launcher.into()),
        }
    }
}

impl Default for SystemBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl Browser for SystemBrowser {
    fn open(&self, url: &str) -> anyhow::Result<()> {
        match &self.launcher {
            Some(launcher) => {
                let parts = shlex::split(launcher).unwrap_or_else(|| vec![launcher.clone()]);
                if parts.is_empty() {
                    open::that(url)?;
                } else {
                    std::process::Command::new(&parts[0])
                        .args(&parts[1..])
                        .arg(url)
                        .spawn()?;
                }
            }
            None => {
                open::that(url)?;
            }
        }
        Ok(())
    }
}

/// Stub browser for testing that records URLs instead of opening them.
#[derive(Debug, Default)]
pub struct StubBrowser {
    /// URLs that were "opened".
    pub urls: std::sync::Mutex<Vec<String>>,
}

impl Browser for StubBrowser {
    fn open(&self, url: &str) -> anyhow::Result<()> {
        let mut urls = self
            .urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        urls.push(url.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_record_urls_in_stub() {
        let browser = StubBrowser::default();
        browser.open("https://github.com").unwrap();
        browser.open("https://github.com/cli/cli").unwrap();

        let urls = browser.urls.lock().unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://github.com");
        assert_eq!(urls[1], "https://github.com/cli/cli");
    }

    #[test]
    fn test_should_start_empty() {
        let browser = StubBrowser::default();
        let urls = browser.urls.lock().unwrap();
        assert!(urls.is_empty());
    }

    #[test]
    fn test_should_create_default_system_browser() {
        let browser = SystemBrowser::new();
        assert!(format!("{browser:?}").contains("None"));
    }

    #[test]
    fn test_should_create_system_browser_with_launcher() {
        let browser = SystemBrowser::with_launcher("firefox");
        assert!(format!("{browser:?}").contains("firefox"));
    }

    #[test]
    fn test_should_have_default_impl() {
        let browser = SystemBrowser::default();
        assert!(format!("{browser:?}").contains("None"));
    }
}
