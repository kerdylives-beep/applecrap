//! Twitch Helix calls for Channel Points song requests: managing the reward
//! the app owns, settling redemptions, and subscribing to new ones over
//! EventSub.
//!
//! Twitch only lets the app that created a reward update it or change its
//! redemptions' status, so the app creates its own reward rather than
//! adopting one the streamer made by hand.

use serde::Deserialize;
use serde_json::json;

const HELIX: &str = "https://api.twitch.tv/helix";

/// Credentials for one Helix call.
pub struct HelixAuth<'a> {
    pub client_id: &'a str,
    pub token: &'a str,
}

#[derive(Debug)]
pub enum HelixError {
    /// The token was rejected; refresh and retry once.
    Unauthorized,
    NotFound,
    /// Usually: the channel isn't Affiliate/Partner, or the reward wasn't
    /// created by this app.
    Forbidden(String),
    Api { status: u16, message: String },
    Network(reqwest::Error),
    /// Couldn't open the EventSub socket, or refresh the token, for want of
    /// a connection.
    Connection(String),
    /// The broadcaster isn't signed in (or the sign-in lapsed).
    SignIn(String),
}

impl HelixError {
    /// Worth retrying later (as opposed to needing the user to act).
    pub fn is_transient(&self) -> bool {
        match self {
            HelixError::Network(_) | HelixError::Connection(_) => true,
            HelixError::Api { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// What to tell the streamer, in terms of what they can do about it.
    pub fn user_message(&self, reward_title: &str) -> String {
        match self {
            HelixError::Forbidden(_) => {
                "Channel Points need a Twitch Affiliate or Partner channel.".to_string()
            }
            HelixError::Api { message, .. } if message.contains("DUPLICATE_REWARD") => format!(
                "Your channel already has a reward called \"{reward_title}\". Pick a different name here."
            ),
            other => other.to_string(),
        }
    }
}

impl std::fmt::Display for HelixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelixError::Unauthorized => write!(f, "Twitch rejected the sign-in"),
            HelixError::NotFound => write!(f, "Twitch couldn't find the reward"),
            HelixError::Forbidden(message) => write!(f, "Twitch refused: {message}"),
            HelixError::Api { status, message } => write!(f, "Twitch error {status}: {message}"),
            HelixError::Network(error) => write!(f, "couldn't reach Twitch: {error}"),
            HelixError::Connection(message) => write!(f, "couldn't reach Twitch: {message}"),
            HelixError::SignIn(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HelixError {}

#[derive(Debug, Clone, Deserialize)]
pub struct Redemption {
    pub id: String,
    pub user_login: String,
    pub user_name: String,
    #[serde(default)]
    pub user_input: String,
    /// RFC 3339.
    pub redeemed_at: String,
}

pub struct RewardSpec<'a> {
    pub title: &'a str,
    pub cost: u32,
    pub prompt: &'a str,
}

#[derive(Deserialize)]
struct DataList<T> {
    data: Vec<T>,
}

#[derive(Deserialize)]
struct CreatedReward {
    id: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
}

fn authed(request: reqwest::RequestBuilder, auth: &HelixAuth<'_>) -> reqwest::RequestBuilder {
    request
        .header("Authorization", format!("Bearer {}", auth.token))
        .header("Client-Id", auth.client_id)
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, HelixError> {
    let response = request.send().await.map_err(HelixError::Network)?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let message = response
        .json::<ErrorBody>()
        .await
        .map(|body| body.message)
        .unwrap_or_default();
    Err(match status.as_u16() {
        401 => HelixError::Unauthorized,
        403 => HelixError::Forbidden(message),
        404 => HelixError::NotFound,
        code => HelixError::Api { status: code, message },
    })
}

fn reward_body(spec: &RewardSpec<'_>) -> serde_json::Value {
    json!({
        "title": spec.title,
        "cost": spec.cost,
        "prompt": spec.prompt,
        "is_user_input_required": true,
        "is_enabled": true,
        "is_paused": false,
        // Settled by the app as each request is accepted or refunded.
        "should_redemptions_skip_request_queue": false,
    })
}

/// Creates the reward and returns its id.
pub async fn create_reward(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    spec: &RewardSpec<'_>,
) -> Result<String, HelixError> {
    let request = http
        .post(format!("{HELIX}/channel_points/custom_rewards"))
        .query(&[("broadcaster_id", broadcaster_id)])
        .json(&reward_body(spec));
    let created: DataList<CreatedReward> = send(authed(request, auth))
        .await?
        .json()
        .await
        .map_err(HelixError::Network)?;
    created
        .data
        .into_iter()
        .next()
        .map(|reward| reward.id)
        .ok_or(HelixError::Api {
            status: 200,
            message: "Twitch didn't return the new reward".to_string(),
        })
}

/// Brings the reward in line with the settings and makes it redeemable.
pub async fn update_reward(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
    spec: &RewardSpec<'_>,
) -> Result<(), HelixError> {
    patch_reward(http, auth, broadcaster_id, reward_id, reward_body(spec)).await
}

/// Hides the reward from viewers (while the feature is off, or nobody is
/// signed in to settle redemptions).
pub async fn disable_reward(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
) -> Result<(), HelixError> {
    patch_reward(http, auth, broadcaster_id, reward_id, json!({ "is_enabled": false })).await
}

async fn patch_reward(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
    body: serde_json::Value,
) -> Result<(), HelixError> {
    let request = http
        .patch(format!("{HELIX}/channel_points/custom_rewards"))
        .query(&[("broadcaster_id", broadcaster_id), ("id", reward_id)])
        .json(&body);
    send(authed(request, auth)).await.map(|_| ())
}

/// Redemptions still waiting to be settled, oldest first — the ones that
/// arrived while the app was closed or the listener was reconnecting.
pub async fn unfulfilled_redemptions(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
) -> Result<Vec<Redemption>, HelixError> {
    let request = http
        .get(format!("{HELIX}/channel_points/custom_rewards/redemptions"))
        .query(&[
            ("broadcaster_id", broadcaster_id),
            ("reward_id", reward_id),
            ("status", "UNFULFILLED"),
            ("sort", "OLDEST"),
            ("first", "50"),
        ]);
    let list: DataList<Redemption> = send(authed(request, auth))
        .await?
        .json()
        .await
        .map_err(HelixError::Network)?;
    Ok(list.data)
}

/// Marks a redemption done, or cancels it — which refunds the viewer.
pub async fn settle_redemption(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
    redemption_id: &str,
    fulfilled: bool,
) -> Result<(), HelixError> {
    let request = http
        .patch(format!("{HELIX}/channel_points/custom_rewards/redemptions"))
        .query(&[
            ("broadcaster_id", broadcaster_id),
            ("reward_id", reward_id),
            ("id", redemption_id),
        ])
        .json(&json!({ "status": if fulfilled { "FULFILLED" } else { "CANCELED" } }));
    send(authed(request, auth)).await.map(|_| ())
}

/// Subscribes an EventSub WebSocket session to new redemptions of the reward.
/// Must happen within 10 seconds of the session's welcome message.
pub async fn subscribe_redemptions(
    http: &reqwest::Client,
    auth: &HelixAuth<'_>,
    broadcaster_id: &str,
    reward_id: &str,
    session_id: &str,
) -> Result<(), HelixError> {
    let request = http.post(format!("{HELIX}/eventsub/subscriptions")).json(&json!({
        "type": "channel.channel_points_custom_reward_redemption.add",
        "version": "1",
        "condition": { "broadcaster_user_id": broadcaster_id, "reward_id": reward_id },
        "transport": { "method": "websocket", "session_id": session_id },
    }));
    send(authed(request, auth)).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_the_errors_a_streamer_can_fix() {
        let forbidden = HelixError::Forbidden("The broadcaster must have partner or affiliate status.".into());
        assert!(forbidden.user_message("Request a song").contains("Affiliate"));

        let duplicate = HelixError::Api {
            status: 400,
            message: "CREATE_CUSTOM_REWARD_DUPLICATE_REWARD".into(),
        };
        assert!(duplicate.user_message("Request a song").contains("\"Request a song\""));
    }

    #[test]
    fn only_network_and_server_errors_are_transient() {
        assert!(HelixError::Api { status: 503, message: String::new() }.is_transient());
        assert!(HelixError::Api { status: 429, message: String::new() }.is_transient());
        assert!(!HelixError::Api { status: 400, message: String::new() }.is_transient());
        assert!(!HelixError::Forbidden(String::new()).is_transient());
        assert!(!HelixError::Unauthorized.is_transient());
    }

    #[test]
    fn parses_helix_redemptions() {
        let list: DataList<Redemption> = serde_json::from_str(
            r#"{"data":[{"broadcaster_name":"b","broadcaster_login":"b","broadcaster_id":"1","id":"r1","user_id":"2","user_login":"viewer","user_name":"Viewer","user_input":"hello adele","status":"UNFULFILLED","redeemed_at":"2026-09-24T18:00:00Z","reward":{"id":"w","title":"Request a song","prompt":"","cost":500}}],"pagination":{}}"#,
        )
        .unwrap();
        assert_eq!(list.data[0].user_input, "hello adele");
        assert_eq!(list.data[0].user_name, "Viewer");
    }
}
