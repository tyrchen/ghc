//! Keyring-based secret storage.
//!
//! Uses OS-native credential storage (Keychain on macOS, etc.).
//! Implements gh's two-slot keyring model:
//! - **Per-user slot**: `keyring.Entry("gh:{hostname}", username)` — stores during login
//! - **Active slot**: `keyring.Entry("gh:{hostname}", "")` — moved during `activate_user`

use std::time::Duration;

use anyhow::{Context, Result};

/// Timeout for keyring operations (matches Go CLI's 3-second timeout).
const KEYRING_TIMEOUT: Duration = Duration::from_secs(3);

/// Run a keyring operation with a timeout to avoid hangs.
fn with_timeout<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = f();
        let _ = tx.send(result);
    });
    rx.recv_timeout(KEYRING_TIMEOUT)
        .map_err(|_| anyhow::anyhow!("keyring operation timed out after 3 seconds"))?
}

/// Build a keyring entry for the active slot (username = "").
fn active_entry(hostname: &str) -> Result<keyring::Entry> {
    let service = format!("gh:{hostname}");
    keyring::Entry::new(&service, "").context("failed to create keyring entry")
}

/// Build a keyring entry for a specific user.
fn user_entry(hostname: &str, username: &str) -> Result<keyring::Entry> {
    let service = format!("gh:{hostname}");
    keyring::Entry::new(&service, username).context("failed to create keyring entry")
}

/// Store a token in the OS keyring active slot (username = "").
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn store_token(hostname: &str, token: &str) -> Result<()> {
    let hostname = hostname.to_string();
    let token = token.to_string();
    with_timeout(move || {
        let entry = active_entry(&hostname)?;
        entry
            .set_password(&token)
            .context("failed to store token in keyring")?;
        Ok(())
    })
}

/// Retrieve a token from the OS keyring active slot.
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn get_token(hostname: &str) -> Result<Option<String>> {
    let hostname = hostname.to_string();
    with_timeout(move || {
        let entry = active_entry(&hostname)?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
        }
    })
}

/// Delete a token from the OS keyring active slot.
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn delete_token(hostname: &str) -> Result<()> {
    let hostname = hostname.to_string();
    with_timeout(move || {
        let entry = active_entry(&hostname)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
        }
    })
}

/// Store a token in the OS keyring per-user slot.
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn store_token_for_user(hostname: &str, username: &str, token: &str) -> Result<()> {
    let hostname = hostname.to_string();
    let username = username.to_string();
    let token = token.to_string();
    with_timeout(move || {
        let entry = user_entry(&hostname, &username)?;
        entry
            .set_password(&token)
            .context("failed to store token in keyring")?;
        Ok(())
    })
}

/// Retrieve a token from the OS keyring per-user slot.
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn get_token_for_user(hostname: &str, username: &str) -> Result<Option<String>> {
    let hostname = hostname.to_string();
    let username = username.to_string();
    with_timeout(move || {
        let entry = user_entry(&hostname, &username)?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
        }
    })
}

/// Delete a token from the OS keyring per-user slot.
///
/// # Errors
///
/// Returns an error if the keyring operation fails or times out.
pub fn delete_token_for_user(hostname: &str, username: &str) -> Result<()> {
    let hostname = hostname.to_string();
    let username = username.to_string();
    with_timeout(move || {
        let entry = user_entry(&hostname, &username)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("keyring error: {e}")),
        }
    })
}
