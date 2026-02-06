//! OAuth device flow for GitHub authentication.
//!
//! Maps from Go's `internal/authflow` package. Supports the device
//! authorization grant with browser opening and clipboard integration.

use serde::Deserialize;
use tracing::info;

use ghc_core::browser::Browser;
use ghc_core::instance;

/// OAuth device code response.
#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    /// Device verification code.
    pub device_code: String,
    /// User-visible code to enter.
    pub user_code: String,
    /// URL to visit for verification.
    pub verification_uri: String,
    /// Expiration in seconds.
    pub expires_in: u64,
    /// Polling interval in seconds.
    pub interval: u64,
}

/// OAuth access token response.
#[derive(Debug, Deserialize)]
pub struct AccessTokenResponse {
    /// The access token.
    pub access_token: Option<String>,
    /// Token type (usually "bearer").
    pub token_type: Option<String>,
    /// OAuth scopes granted.
    pub scope: Option<String>,
    /// Error code if the request failed.
    pub error: Option<String>,
    /// Error description.
    pub error_description: Option<String>,
}

/// Result of a completed OAuth device flow.
#[derive(Debug)]
pub struct AuthFlowResult {
    /// The access token obtained.
    pub token: String,
    /// The username of the authenticated user (if retrieved during flow).
    pub username: String,
}

/// Default OAuth scopes requested during login.
pub const DEFAULT_SCOPES: &[&str] = &["repo", "read:org", "gist"];

/// The GHC OAuth client ID.
///
/// This should be set to a registered OAuth application's client ID.
/// For development, GitHub CLI's client ID can be used temporarily.
const CLIENT_ID: &str = "178c6fc778ccc68e1d6a";

/// Execute the full OAuth device flow with browser integration.
///
/// Steps:
/// 1. Request a device code from GitHub
/// 2. Display the user code and open browser (or copy to clipboard)
/// 3. Poll for the access token
/// 4. Retrieve the authenticated username
///
/// # Errors
///
/// Returns an error if any step of the flow fails.
pub async fn auth_flow(
    client: &reqwest::Client,
    hostname: &str,
    scopes: &[&str],
    browser: &dyn Browser,
    copy_to_clipboard: bool,
    write_status: &mut dyn std::io::Write,
) -> anyhow::Result<AuthFlowResult> {
    let effective_scopes = if scopes.is_empty() {
        DEFAULT_SCOPES.to_vec()
    } else {
        scopes.to_vec()
    };

    let device_code = request_device_code(client, hostname, &effective_scopes).await?;

    writeln!(write_status)?;
    writeln!(
        write_status,
        "! First copy your one-time code: {}",
        device_code.user_code,
    )?;

    if copy_to_clipboard {
        if let Err(e) = copy_to_system_clipboard(&device_code.user_code) {
            writeln!(write_status, "! Failed to copy to clipboard: {e}",)?;
        } else {
            writeln!(write_status, "! Copied to clipboard.")?;
        }
    }

    writeln!(
        write_status,
        "- Press Enter to open {} in your browser...",
        device_code.verification_uri,
    )?;

    // Try to open the browser
    if let Err(e) = browser.open(&device_code.verification_uri) {
        writeln!(write_status, "! Failed to open browser: {e}",)?;
        writeln!(
            write_status,
            "  Open this URL manually: {}",
            device_code.verification_uri,
        )?;
    }

    let token = poll_access_token(client, hostname, &device_code).await?;

    // Get the username for the authenticated token
    let username = get_username(client, hostname, &token).await?;

    Ok(AuthFlowResult { token, username })
}

/// Initiate the OAuth device flow.
///
/// # Errors
///
/// Returns an error if the device code request fails.
pub async fn request_device_code(
    client: &reqwest::Client,
    hostname: &str,
    scopes: &[&str],
) -> anyhow::Result<DeviceCodeResponse> {
    let normalized = instance::normalize_hostname(hostname);
    let url = if instance::is_github_com(&normalized) {
        "https://github.com/login/device/code".to_string()
    } else {
        format!("https://{normalized}/login/device/code")
    };

    let scope = scopes.join(" ");

    let resp = client
        .post(&url)
        .header("Accept", "application/json")
        .form(&[("client_id", CLIENT_ID), ("scope", &scope)])
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await?;
        anyhow::bail!("device code request failed: {text}");
    }

    let code: DeviceCodeResponse = resp.json().await?;
    info!(
        verification_uri = %code.verification_uri,
        user_code = %code.user_code,
        "Device code obtained"
    );
    Ok(code)
}

/// Poll for the access token after user completes verification.
///
/// # Errors
///
/// Returns an error if polling fails or the code expires.
pub async fn poll_access_token(
    client: &reqwest::Client,
    hostname: &str,
    device_code: &DeviceCodeResponse,
) -> anyhow::Result<String> {
    let normalized = instance::normalize_hostname(hostname);
    let url = if instance::is_github_com(&normalized) {
        "https://github.com/login/oauth/access_token".to_string()
    } else {
        format!("https://{normalized}/login/oauth/access_token")
    };

    let interval = std::time::Duration::from_secs(device_code.interval.max(5));
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(device_code.expires_in);

    loop {
        tokio::time::sleep(interval).await;

        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("device code expired");
        }

        let resp = client
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", CLIENT_ID),
                ("device_code", &device_code.device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;

        let token_resp: AccessTokenResponse = resp.json().await?;

        if let Some(token) = token_resp.access_token {
            return Ok(token);
        }

        match token_resp.error.as_deref() {
            Some("authorization_pending") | None => {}
            Some("slow_down") => {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Some(err) => {
                let desc = token_resp.error_description.unwrap_or_default();
                anyhow::bail!("OAuth error: {err} - {desc}");
            }
        }
    }
}

/// Retrieve the authenticated username for a token via GraphQL.
async fn get_username(
    client: &reqwest::Client,
    hostname: &str,
    token: &str,
) -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct Wrapper {
        data: DataInner,
    }
    #[derive(Deserialize)]
    struct DataInner {
        viewer: ViewerLogin,
    }
    #[derive(Deserialize)]
    struct ViewerLogin {
        login: String,
    }

    let url = instance::graphql_url(hostname);
    let body = serde_json::json!({
        "query": "query UserCurrent { viewer { login } }"
    });

    let resp = client
        .post(&url)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await?;
        anyhow::bail!("failed to retrieve username: {text}");
    }

    let wrapper: Wrapper = resp.json().await?;
    Ok(wrapper.data.viewer.login)
}

/// Copy text to the system clipboard.
fn copy_to_system_clipboard(text: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let mut child = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        // Try xclip first, then xsel
        let result = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn();
        match result {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()?;
                return Ok(());
            }
            Err(_) => {
                let mut child = std::process::Command::new("xsel")
                    .args(["--clipboard", "--input"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()?;
                return Ok(());
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::io::Write;
        let mut child = std::process::Command::new("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        child.wait()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        anyhow::bail!("clipboard not supported on this platform")
    }
}
