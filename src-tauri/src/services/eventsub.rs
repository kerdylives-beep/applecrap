//! Twitch EventSub over WebSocket: just enough to hear Channel Points
//! redemptions as they happen.
//!
//! Protocol notes (Twitch docs): the server opens with `session_welcome`
//! carrying the session id, which must be used to subscribe within 10
//! seconds. Silence longer than the keepalive timeout means the connection
//! is dead. `session_reconnect` hands over a new URL; subscriptions carry
//! over as long as the old socket stays open until the new one is welcomed.

use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{
    tungstenite::{Error as WsError, Message},
    MaybeTlsStream, WebSocketStream,
};

use crate::services::channel_points::Redemption;

/// Asks for a longer keepalive than the 10s default: fewer wakeups, and
/// still quick to notice a dead connection.
pub const EVENTSUB_URL: &str = "wss://eventsub.wss.twitch.tv/ws?keepalive_timeout_seconds=30";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);
/// Grace on top of the keepalive timeout before calling a socket dead.
const STALE_GRACE: Duration = Duration::from_secs(10);

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
pub enum EventSubMessage {
    Welcome { session_id: String, keepalive: Duration },
    Keepalive,
    Redemption(Redemption),
    Reconnect(String),
    Revoked(String),
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionEnd {
    Reconnect(String),
    Revoked(String),
    Closed,
    Stale,
    Io(String),
}

#[derive(Deserialize)]
struct Envelope {
    metadata: Metadata,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Deserialize)]
struct Metadata {
    message_type: String,
}

pub fn parse(text: &str) -> Option<EventSubMessage> {
    let envelope: Envelope = serde_json::from_str(text).ok()?;
    let payload = &envelope.payload;
    let message = match envelope.metadata.message_type.as_str() {
        "session_welcome" => EventSubMessage::Welcome {
            session_id: payload["session"]["id"].as_str()?.to_string(),
            keepalive: Duration::from_secs(
                payload["session"]["keepalive_timeout_seconds"].as_u64().unwrap_or(10),
            ),
        },
        "session_keepalive" => EventSubMessage::Keepalive,
        "notification" => match serde_json::from_value(payload["event"].clone()) {
            Ok(redemption) => EventSubMessage::Redemption(redemption),
            Err(_) => EventSubMessage::Other,
        },
        "session_reconnect" => {
            EventSubMessage::Reconnect(payload["session"]["reconnect_url"].as_str()?.to_string())
        }
        "revocation" => EventSubMessage::Revoked(
            payload["subscription"]["status"]
                .as_str()
                .unwrap_or("revoked")
                .to_string(),
        ),
        _ => EventSubMessage::Other,
    };
    Some(message)
}

/// Connects and waits for the welcome. Returns the socket, its session id
/// and keepalive timeout.
pub async fn open(url: &str) -> Result<(Socket, String, Duration)> {
    let (mut socket, _) = timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| anyhow!("timed out connecting to Twitch EventSub"))??;

    let welcome = timeout(WELCOME_TIMEOUT, async {
        while let Some(frame) = socket.next().await {
            if let Message::Text(text) = frame? {
                if let Some(EventSubMessage::Welcome { session_id, keepalive }) = parse(text.as_str()) {
                    return Ok((session_id, keepalive));
                }
            }
        }
        Err(anyhow!("Twitch EventSub closed before welcoming the session"))
    })
    .await
    .map_err(|_| anyhow!("Twitch EventSub never welcomed the session"))??;

    Ok((socket, welcome.0, welcome.1))
}

/// Reads until the session ends, passing each redemption to `on_redemption`.
/// Generic over the stream so tests can drive it without a network.
pub async fn read_until_end<S>(
    socket: &mut S,
    keepalive: Duration,
    mut on_redemption: impl FnMut(Redemption),
) -> SessionEnd
where
    S: Stream<Item = Result<Message, WsError>> + Unpin,
{
    let stale_after = keepalive + STALE_GRACE;
    loop {
        let frame = match timeout(stale_after, socket.next()).await {
            Err(_) => return SessionEnd::Stale,
            Ok(None) => return SessionEnd::Closed,
            Ok(Some(Err(error))) => return SessionEnd::Io(error.to_string()),
            Ok(Some(Ok(frame))) => frame,
        };
        let text = match frame {
            Message::Text(text) => text,
            Message::Close(_) => return SessionEnd::Closed,
            // Pings are answered by tungstenite; any frame counts as life.
            _ => continue,
        };
        match parse(text.as_str()) {
            Some(EventSubMessage::Redemption(redemption)) => on_redemption(redemption),
            Some(EventSubMessage::Reconnect(url)) => return SessionEnd::Reconnect(url),
            Some(EventSubMessage::Revoked(reason)) => return SessionEnd::Revoked(reason),
            _ => {}
        }
    }
}

/// Only follow reconnect URLs that point back at Twitch's EventSub servers.
pub fn is_twitch_eventsub_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .map(|parsed| {
            parsed.scheme() == "wss"
                && parsed
                    .host_str()
                    .is_some_and(|host| host == "eventsub.wss.twitch.tv" || host.ends_with(".twitch.tv"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use futures_util::stream;

    use super::*;

    const WELCOME: &str = r#"{"metadata":{"message_id":"m1","message_type":"session_welcome","message_timestamp":"2026-09-24T18:00:00Z"},"payload":{"session":{"id":"AgoQ","status":"connected","connected_at":"2026-09-24T18:00:00Z","keepalive_timeout_seconds":30,"reconnect_url":null}}}"#;
    const REDEMPTION: &str = r#"{"metadata":{"message_id":"m2","message_type":"notification","message_timestamp":"2026-09-24T18:00:01Z","subscription_type":"channel.channel_points_custom_reward_redemption.add","subscription_version":"1"},"payload":{"subscription":{"id":"s","status":"enabled","type":"channel.channel_points_custom_reward_redemption.add","version":"1","condition":{"broadcaster_user_id":"1","reward_id":"w"},"transport":{"method":"websocket","session_id":"AgoQ"},"created_at":"2026-09-24T18:00:00Z","cost":0},"event":{"id":"r1","broadcaster_user_id":"1","broadcaster_user_login":"b","broadcaster_user_name":"B","user_id":"2","user_login":"viewer","user_name":"Viewer","user_input":"never gonna give you up","status":"unfulfilled","reward":{"id":"w","title":"Request a song","cost":500,"prompt":""},"redeemed_at":"2026-09-24T18:00:01Z"}}}"#;
    const RECONNECT: &str = r#"{"metadata":{"message_id":"m3","message_type":"session_reconnect","message_timestamp":"2026-09-24T18:00:02Z"},"payload":{"session":{"id":"AgoQ","status":"reconnecting","keepalive_timeout_seconds":null,"reconnect_url":"wss://eventsub.wss.twitch.tv/ws?id=abc","connected_at":"2026-09-24T18:00:00Z"}}}"#;
    const KEEPALIVE: &str = r#"{"metadata":{"message_id":"m4","message_type":"session_keepalive","message_timestamp":"2026-09-24T18:00:03Z"},"payload":{}}"#;
    const REVOKED: &str = r#"{"metadata":{"message_id":"m5","message_type":"revocation","message_timestamp":"2026-09-24T18:00:04Z","subscription_type":"channel.channel_points_custom_reward_redemption.add","subscription_version":"1"},"payload":{"subscription":{"id":"s","status":"authorization_revoked","type":"channel.channel_points_custom_reward_redemption.add","version":"1","condition":{},"transport":{"method":"websocket","session_id":"AgoQ"},"created_at":"2026-09-24T18:00:00Z","cost":0}}}"#;

    fn text(value: &str) -> Result<Message, WsError> {
        Ok(Message::text(value))
    }

    #[test]
    fn parses_session_messages() {
        match parse(WELCOME) {
            Some(EventSubMessage::Welcome { session_id, keepalive }) => {
                assert_eq!(session_id, "AgoQ");
                assert_eq!(keepalive, Duration::from_secs(30));
            }
            other => panic!("unexpected {other:?}"),
        }
        match parse(REDEMPTION) {
            Some(EventSubMessage::Redemption(redemption)) => {
                assert_eq!(redemption.id, "r1");
                assert_eq!(redemption.user_name, "Viewer");
                assert_eq!(redemption.user_input, "never gonna give you up");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(parse(KEEPALIVE), Some(EventSubMessage::Keepalive)));
        assert!(matches!(parse(REVOKED), Some(EventSubMessage::Revoked(reason)) if reason == "authorization_revoked"));
        assert!(parse("not json").is_none());
    }

    #[tokio::test]
    async fn delivers_redemptions_then_follows_reconnect() {
        let mut socket = stream::iter(vec![text(KEEPALIVE), text(REDEMPTION), text(RECONNECT), text(REDEMPTION)]);
        let mut seen = Vec::new();
        let end = read_until_end(&mut socket, Duration::from_secs(30), |redemption| seen.push(redemption.id)).await;
        assert_eq!(end, SessionEnd::Reconnect("wss://eventsub.wss.twitch.tv/ws?id=abc".to_string()));
        assert_eq!(seen, vec!["r1"]);
    }

    #[tokio::test]
    async fn reports_revocation_and_closure() {
        let mut revoked = stream::iter(vec![text(REVOKED)]);
        assert_eq!(
            read_until_end(&mut revoked, Duration::from_secs(30), |_| {}).await,
            SessionEnd::Revoked("authorization_revoked".to_string())
        );
        let mut closed = stream::iter(Vec::<Result<Message, WsError>>::new());
        assert_eq!(read_until_end(&mut closed, Duration::from_secs(30), |_| {}).await, SessionEnd::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn silent_socket_goes_stale() {
        let mut silent = stream::pending::<Result<Message, WsError>>();
        assert_eq!(read_until_end(&mut silent, Duration::from_secs(30), |_| {}).await, SessionEnd::Stale);
    }

    #[test]
    fn only_follows_twitch_reconnect_urls() {
        assert!(is_twitch_eventsub_url("wss://eventsub.wss.twitch.tv/ws?id=abc"));
        assert!(!is_twitch_eventsub_url("wss://evil.example/ws"));
        assert!(!is_twitch_eventsub_url("ws://eventsub.wss.twitch.tv/ws"));
        assert!(!is_twitch_eventsub_url("wss://twitch.tv.evil.example/ws"));
    }
}
