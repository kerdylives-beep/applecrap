//! Twitch sign-in via the device-code flow, plus token refresh, validation
//! and revocation.
//!
//! The app is a "public" Twitch client: no client secret exists or is
//! needed. The user signs in by entering a short code at twitch.tv/activate,
//! which replaces pasting an `oauth:` token from a third-party generator
//! (the one most bots relied on has been discontinued).
//!
//! Token facts that shape the callers (Twitch docs, 2026): access tokens
//! last about four hours; public-client refresh tokens are ONE-TIME USE and
//! lapse after 30 days unused — so refreshes must never run concurrently for
//! the same account, and each new refresh token must be saved immediately.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::models::AuthSlot;

/// Client ID of the AppleCrap application registered at
/// dev.twitch.tv/console (client type: Public). It is not a secret.
/// A build can also supply one via the APPLECRAP_TWITCH_CLIENT_ID env var.
const CLIENT_ID: &str = "jjnrf97l1twk6ebnlgih5mvgpbsx9d";

const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";
const REVOKE_URL: &str = "https://id.twitch.tv/oauth2/revoke";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

pub fn client_id() -> Option<&'static str> {
    let id = option_env!("APPLECRAP_TWITCH_CLIENT_ID").unwrap_or(CLIENT_ID).trim();
    (!id.is_empty()).then_some(id)
}

/// Scopes requested per account. The broadcaster account also asks for
/// Channel Points access up front so enabling that feature later needs no
/// second sign-in; both can chat, so either can serve as the bot.
pub fn scopes_for(slot: AuthSlot) -> &'static str {
    match slot {
        AuthSlot::Bot => "chat:read chat:edit",
        AuthSlot::Broadcaster => {
            "chat:read chat:edit channel:read:redemptions channel:manage:redemptions"
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds until the code expires.
    pub expires_in: i64,
    /// Minimum seconds between token polls.
    pub interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenGrant {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Vec<String>,
}

#[derive(Debug)]
pub enum TokenPoll {
    /// The user hasn't entered the code yet.
    Pending,
    /// Polling too fast; back off.
    SlowDown,
    Granted(TokenGrant),
    Expired,
    Failed(String),
}

#[derive(Debug)]
pub enum RefreshError {
    /// The refresh token was rejected (used, revoked, or lapsed). The user
    /// must sign in again.
    Invalid,
    /// Network or server trouble; the tokens are still good to retry.
    Transient(anyhow::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenInfo {
    pub login: String,
    pub user_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Deserialize)]
struct TwitchError {
    #[serde(default)]
    message: String,
}

pub async fn request_device_code(
    http: &reqwest::Client,
    client_id: &str,
    scopes: &str,
) -> Result<DeviceAuthorization> {
    http.post(DEVICE_URL)
        .form(&[("client_id", client_id), ("scopes", scopes)])
        .send()
        .await?
        .error_for_status()
        .context("Twitch refused to start sign-in")?
        .json()
        .await
        .context("Twitch sent an unexpected sign-in response")
}

pub async fn poll_device_token(
    http: &reqwest::Client,
    client_id: &str,
    scopes: &str,
    device_code: &str,
) -> Result<TokenPoll> {
    let response = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("scopes", scopes),
            ("device_code", device_code),
            ("grant_type", DEVICE_GRANT),
        ])
        .send()
        .await?;

    if response.status().is_success() {
        return Ok(TokenPoll::Granted(response.json().await?));
    }

    let message = response
        .json::<TwitchError>()
        .await
        .map(|error| error.message)
        .unwrap_or_default();
    Ok(classify_poll_error(&message))
}

fn classify_poll_error(message: &str) -> TokenPoll {
    let lower = message.to_ascii_lowercase();
    if lower.contains("authorization_pending") {
        TokenPoll::Pending
    } else if lower.contains("slow_down") {
        TokenPoll::SlowDown
    } else if lower.contains("invalid device code") || lower.contains("expired") {
        TokenPoll::Expired
    } else if message.is_empty() {
        TokenPoll::Failed("Twitch rejected the sign-in.".to_string())
    } else {
        TokenPoll::Failed(message.to_string())
    }
}

pub async fn refresh(
    http: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> std::result::Result<TokenGrant, RefreshError> {
    let response = http
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(|error| RefreshError::Transient(error.into()))?;

    let status = response.status();
    if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RefreshError::Invalid);
    }
    // Other failures (5xx, rate limits, dropped connections) stay as
    // `reqwest::Error`, which the chat supervisor treats as "retry later".
    let response = response
        .error_for_status()
        .map_err(|error| RefreshError::Transient(error.into()))?;
    response
        .json()
        .await
        .map_err(|error| RefreshError::Transient(error.into()))
}

/// Twitch requires apps to validate tokens at startup and hourly. `None`
/// means the token is no longer valid.
pub async fn validate(http: &reqwest::Client, access_token: &str) -> Result<Option<TokenInfo>> {
    let response = http
        .get(VALIDATE_URL)
        .header("Authorization", format!("OAuth {access_token}"))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(None);
    }
    Ok(Some(
        response
            .error_for_status()
            .context("Twitch token validation failed")?
            .json()
            .await?,
    ))
}

pub async fn revoke(http: &reqwest::Client, client_id: &str, access_token: &str) -> Result<()> {
    http.post(REVOKE_URL)
        .form(&[("client_id", client_id), ("token", access_token)])
        .send()
        .await?
        .error_for_status()
        .map(|_| ())
        .map_err(|error| anyhow!("revoking the token failed: {error}"))
}

/// Only ever open Twitch's own activation page from a sign-in prompt.
pub fn is_twitch_activation_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .map(|parsed| {
            parsed.scheme() == "https"
                && matches!(parsed.host_str(), Some("www.twitch.tv") | Some("twitch.tv"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_device_poll_errors() {
        assert!(matches!(classify_poll_error("authorization_pending"), TokenPoll::Pending));
        assert!(matches!(classify_poll_error("slow_down"), TokenPoll::SlowDown));
        assert!(matches!(classify_poll_error("invalid device code"), TokenPoll::Expired));
        assert!(matches!(classify_poll_error("something else"), TokenPoll::Failed(_)));
        assert!(matches!(classify_poll_error(""), TokenPoll::Failed(_)));
    }

    #[test]
    fn broadcaster_can_manage_rewards_and_both_can_chat() {
        assert!(scopes_for(AuthSlot::Broadcaster).contains("channel:manage:redemptions"));
        assert!(scopes_for(AuthSlot::Bot).contains("chat:edit"));
        assert!(scopes_for(AuthSlot::Broadcaster).contains("chat:edit"));
    }

    #[test]
    fn only_twitch_activation_pages_are_openable() {
        assert!(is_twitch_activation_url("https://www.twitch.tv/activate"));
        assert!(is_twitch_activation_url("https://twitch.tv/activate?device-code=ABCD"));
        assert!(!is_twitch_activation_url("http://www.twitch.tv/activate"));
        assert!(!is_twitch_activation_url("https://twitch.tv.evil.example/activate"));
        assert!(!is_twitch_activation_url("file:///C:/Windows/System32/calc.exe"));
    }

    #[test]
    fn parses_twitch_token_response() {
        let grant: TokenGrant = serde_json::from_str(
            r#"{"access_token":"a","expires_in":14124,"refresh_token":"r","scope":["chat:read"],"token_type":"bearer"}"#,
        )
        .unwrap();
        assert_eq!(grant.expires_in, 14124);
        assert_eq!(grant.scope, vec!["chat:read"]);
    }
}
