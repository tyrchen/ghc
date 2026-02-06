//! Keyring-based secret storage.
//!
//! Uses OS-native credential storage (Keychain on macOS, etc.).

use anyhow::{Context, Result};

/// Store a token in the OS keyring.
///
/// # Errors
///
/// Returns an error if the keyring operation fails.
pub fn store_token(hostname: &str, token: &str) -> Result<()> {
    let service = format!("ghc:{hostname}");
    let entry =
        keyring::Entry::new(&service, "oauth_token").context("failed to create keyring entry")?;
    entry
        .set_password(token)
        .context("failed to store token in keyring")?;
    Ok(())
}

/// Retrieve a token from the OS keyring.
///
/// # Errors
///
/// Returns an error if the keyring operation fails.
pub fn get_token(hostname: &str) -> Result<Option<String>> {
    let service = format!("ghc:{hostname}");
    let entry =
        keyring::Entry::new(&service, "oauth_token").context("failed to create keyring entry")?;

    match entry.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
    }
}

/// Delete a token from the OS keyring.
///
/// # Errors
///
/// Returns an error if the keyring operation fails.
pub fn delete_token(hostname: &str) -> Result<()> {
    let service = format!("ghc:{hostname}");
    let entry =
        keyring::Entry::new(&service, "oauth_token").context("failed to create keyring entry")?;

    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
    }
}
