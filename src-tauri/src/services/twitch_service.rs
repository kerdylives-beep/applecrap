//! Twitch chat: a supervised IRC connection that looks after itself.
//!
//! Three pieces with separate jobs:
//! - the **supervisor** owns the connection's lifetime: it reconnects with
//!   backoff, honours Twitch's `RECONNECT`, and stops only for problems a
//!   retry cannot fix (rejected credentials);
//! - the **session loop** speaks the protocol and nothing else — PING/PONG,
//!   dead-connection detection, rate-limited writes — so it never blocks on
//!   app work and can be tested against an in-memory stream;
//! - the **worker** runs chat commands one at a time, in order, off the read
//!   loop. A slow song lookup used to stall the loop, so PINGs went
//!   unanswered and Twitch dropped the bot.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::mpsc,
    time::{timeout, Instant},
};
use tokio_native_tls::{native_tls, TlsConnector};

use crate::{
    app::AppContext,
    models::{BotConnectionState, LogLevel, ProbeSnapshot},
    services::{irc, queue_engine},
};

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6697;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CHAT_EVENT_CAPACITY: usize = 256;
/// Replies waiting on the rate limiter beyond this are dropped oldest-first:
/// during a flood, answering the newest requests beats answering stale ones.
const MAX_PENDING_REPLIES: usize = 12;
/// A reply that could not be sent within this window is no longer useful.
const MAX_REPLY_AGE: Duration = Duration::from_secs(20);
/// Twitch rejects messages over 500 characters.
const MAX_CHAT_CHARS: usize = 480;

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Lines the app wants written to the IRC connection.
pub enum Outbound {
    /// A protocol line written immediately (e.g. `QUIT`). Includes CRLF.
    Raw(String),
    /// A chat reply: rate-limited, and dropped if it goes stale.
    Chat { text: String, queued_at: Instant },
}

impl Outbound {
    pub fn chat(text: impl Into<String>) -> Self {
        Outbound::Chat {
            text: text.into(),
            queued_at: Instant::now(),
        }
    }
}

/// Who the bot logs in as. Resolved fresh before every connection attempt so
/// a refreshed token is always the one used.
#[derive(Clone, Debug)]
pub struct ChatIdentity {
    pub login: String,
    /// IRC `PASS` value, including the `oauth:` prefix.
    pub password: String,
    /// Whether a failed login can be recovered by refreshing the token.
    pub refreshable: bool,
}

pub async fn connect(context: Arc<AppContext>) -> Result<()> {
    let identity = context.chat_identity().await?;
    let settings = context.current_settings().await;
    let channel = settings
        .twitch
        .channel
        .trim()
        .trim_start_matches('#')
        .to_lowercase();
    if channel.is_empty() {
        return Err(anyhow!("Enter the Twitch channel the bot should join."));
    }

    context.abort_twitch_connection().await;
    context
        .update_bot_status(
            BotConnectionState::Connecting,
            "Connecting",
            format!("Connecting to Twitch chat for #{channel}..."),
            Some(channel.clone()),
        )
        .await;
    context
        .add_log(
            LogLevel::Info,
            format!(
                "Connecting Twitch bot as @{} to #{channel} (listening for {}, !remove, !song, !queue, and !skip).",
                identity.login,
                settings.twitch.request_command.trim()
            ),
        )
        .await;

    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<Outbound>();
    let (event_tx, event_rx) = mpsc::channel::<SessionEvent>(CHAT_EVENT_CAPACITY);

    // The worker exits on its own once the supervisor (which owns `event_tx`)
    // is aborted, so only the supervisor's handle needs tracking.
    tauri::async_runtime::spawn(event_worker(
        Arc::clone(&context),
        event_rx,
        outbound_tx.clone(),
        channel.clone(),
    ));
    let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    let supervisor = tauri::async_runtime::spawn(supervise(
        Arc::clone(&context),
        connection_id,
        channel,
        outbound_rx,
        event_tx,
    ));

    context
        .register_twitch_connection(connection_id, outbound_tx, supervisor)
        .await;
    Ok(())
}

pub async fn disconnect(context: Arc<AppContext>) -> Result<()> {
    context.abort_twitch_connection().await;
    let channel = context.current_settings().await.twitch.channel;
    context
        .update_bot_status(
            BotConnectionState::Disconnected,
            "Disconnected",
            "Bot is offline.",
            Some(channel),
        )
        .await;
    Ok(())
}

// --- Supervisor ---------------------------------------------------------------

async fn supervise(
    context: Arc<AppContext>,
    connection_id: u64,
    channel: String,
    mut outbound_rx: mpsc::UnboundedReceiver<Outbound>,
    event_tx: mpsc::Sender<SessionEvent>,
) {
    let mut attempt: u32 = 0;
    let mut refreshed_after_auth_failure = false;

    loop {
        let identity = match context.chat_identity().await {
            Ok(identity) => identity,
            Err(error) => {
                stop_with_error(&context, connection_id, &channel, error.to_string()).await;
                return;
            }
        };

        let outcome = match open_stream().await {
            Ok(stream) => {
                session_loop(
                    stream,
                    &identity,
                    &channel,
                    &mut outbound_rx,
                    &event_tx,
                    Timing::LIVE,
                )
                .await
            }
            Err(error) => SessionOutcome {
                end: SessionEnd::Io(error.to_string()),
                welcomed: false,
            },
        };

        if outcome.welcomed {
            attempt = 0;
            refreshed_after_auth_failure = false;
        }

        let reason = match outcome.end {
            SessionEnd::AuthFailed(message) => {
                // A signed-in token may just have expired: refresh once and
                // retry. A second failure (or a pasted token) needs the user.
                if identity.refreshable && !refreshed_after_auth_failure {
                    refreshed_after_auth_failure = true;
                    if context.force_refresh_chat_token().await.is_ok() {
                        context
                            .add_log(LogLevel::Info, "Twitch token refreshed; reconnecting.")
                            .await;
                        continue;
                    }
                }
                stop_with_error(&context, connection_id, &channel, message).await;
                return;
            }
            SessionEnd::Reconnect => {
                context
                    .add_log(
                        LogLevel::Info,
                        "Twitch asked the bot to reconnect (server maintenance). Reconnecting now.",
                    )
                    .await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            SessionEnd::Closed => "Twitch closed the connection".to_string(),
            SessionEnd::Stale => "Twitch stopped responding".to_string(),
            SessionEnd::Io(error) => format!("Connection problem: {error}"),
        };

        attempt = attempt.saturating_add(1);
        let delay = backoff_delay(attempt);
        context
            .update_bot_status(
                BotConnectionState::Connecting,
                "Reconnecting",
                format!("{reason}. Retrying in {}s.", delay.as_secs()),
                Some(channel.clone()),
            )
            .await;
        context
            .add_log(
                LogLevel::Warn,
                format!("{reason}; reconnecting in {}s.", delay.as_secs()),
            )
            .await;
        tokio::time::sleep(delay).await;
    }
}

async fn stop_with_error(
    context: &Arc<AppContext>,
    connection_id: u64,
    channel: &str,
    message: String,
) {
    context
        .update_bot_status(
            BotConnectionState::Error,
            "Connection error",
            message.clone(),
            Some(channel.to_string()),
        )
        .await;
    context
        .add_log(LogLevel::Error, format!("Twitch bot stopped: {message}"))
        .await;
    context.clear_twitch_connection(connection_id).await;
}

/// 2s, 4s, 8s, 16s, 32s, then 60s between attempts.
fn backoff_delay(attempt: u32) -> Duration {
    let seconds = 2_u64.saturating_pow(attempt.clamp(1, 6)).min(60);
    Duration::from_secs(seconds)
}

async fn open_stream() -> Result<tokio_native_tls::TlsStream<TcpStream>> {
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((IRC_HOST, IRC_PORT)))
        .await
        .map_err(|_| anyhow!("timed out connecting to Twitch"))??;
    let connector = TlsConnector::from(native_tls::TlsConnector::builder().build()?);
    let tls = timeout(CONNECT_TIMEOUT, connector.connect(IRC_HOST, tcp))
        .await
        .map_err(|_| anyhow!("timed out securing the Twitch connection"))??;
    Ok(tls)
}

// --- Session loop -------------------------------------------------------------

#[derive(Clone, Copy)]
struct Timing {
    /// How often to PING Twitch so a dead connection is noticed.
    keepalive_every: Duration,
    /// Silence longer than this means the connection is gone, even though the
    /// socket has not reported an error (common after sleep or a Wi-Fi drop).
    stale_after: Duration,
}

impl Timing {
    const LIVE: Timing = Timing {
        keepalive_every: Duration::from_secs(60),
        stale_after: Duration::from_secs(150),
    };
}

#[derive(Debug)]
enum SessionEnd {
    Reconnect,
    Closed,
    Stale,
    AuthFailed(String),
    Io(String),
}

#[derive(Debug)]
struct SessionOutcome {
    end: SessionEnd,
    welcomed: bool,
}

pub enum SessionEvent {
    Welcome,
    Chat(ChatMessage),
}

async fn session_loop<S>(
    stream: S,
    identity: &ChatIdentity,
    channel: &str,
    outbound_rx: &mut mpsc::UnboundedReceiver<Outbound>,
    events: &mpsc::Sender<SessionEvent>,
    timing: Timing,
) -> SessionOutcome
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read_half, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(read_half).lines();
    let mut welcomed = false;

    let end = |end: SessionEnd, welcomed: bool| SessionOutcome { end, welcomed };

    let handshake = format!(
        "CAP REQ :twitch.tv/tags twitch.tv/commands\r\nPASS {}\r\nNICK {}\r\nJOIN #{}\r\n",
        identity.password, identity.login, channel
    );
    if let Err(error) = send_line(&mut writer, &handshake).await {
        return end(SessionEnd::Io(error.to_string()), false);
    }

    let mut last_inbound = Instant::now();
    let mut keepalive = tokio::time::interval(timing.keepalive_every);
    keepalive.tick().await; // the first tick fires immediately
    let mut limiter = ChatRateLimiter::twitch_default();
    let mut pending: VecDeque<(String, Instant)> = VecDeque::new();

    loop {
        let send_at = pending
            .front()
            .map(|_| limiter.next_ready(Instant::now()));

        tokio::select! {
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(line)) => line,
                    Ok(None) => return end(SessionEnd::Closed, welcomed),
                    Err(error) => return end(SessionEnd::Io(error.to_string()), welcomed),
                };
                last_inbound = Instant::now();

                match classify(&line, &identity.login) {
                    Inbound::Ping(payload) => {
                        let pong = format!("PONG :{}\r\n", sanitize_irc_text(&payload));
                        if let Err(error) = send_line(&mut writer, &pong).await {
                            return end(SessionEnd::Io(error.to_string()), welcomed);
                        }
                    }
                    Inbound::Welcome => {
                        welcomed = true;
                        let _ = events.try_send(SessionEvent::Welcome);
                    }
                    Inbound::AuthFailed(message) => {
                        return end(SessionEnd::AuthFailed(message), welcomed);
                    }
                    Inbound::Reconnect => return end(SessionEnd::Reconnect, welcomed),
                    Inbound::Chat(message) => {
                        // Never block the read loop on app work; a full queue
                        // during a flood drops the message instead.
                        let _ = events.try_send(SessionEvent::Chat(message));
                    }
                    Inbound::Other => {}
                }
            }
            outbound = outbound_rx.recv() => match outbound {
                Some(Outbound::Raw(line)) => {
                    if let Err(error) = send_line(&mut writer, &line).await {
                        return end(SessionEnd::Io(error.to_string()), welcomed);
                    }
                }
                Some(Outbound::Chat { text, queued_at }) => {
                    if pending.len() >= MAX_PENDING_REPLIES {
                        pending.pop_front();
                    }
                    pending.push_back((text, queued_at));
                }
                None => return end(SessionEnd::Closed, welcomed),
            },
            _ = keepalive.tick() => {
                if last_inbound.elapsed() >= timing.stale_after {
                    return end(SessionEnd::Stale, welcomed);
                }
                if let Err(error) = send_line(&mut writer, "PING :tmi.twitch.tv\r\n").await {
                    return end(SessionEnd::Io(error.to_string()), welcomed);
                }
            }
            _ = tokio::time::sleep_until(send_at.unwrap_or_else(Instant::now)), if send_at.is_some() => {
                if let Some((text, queued_at)) = pending.pop_front() {
                    if queued_at.elapsed() > MAX_REPLY_AGE {
                        continue;
                    }
                    let line = format_privmsg(channel, &text);
                    if let Err(error) = send_line(&mut writer, &line).await {
                        return end(SessionEnd::Io(error.to_string()), welcomed);
                    }
                    limiter.record(Instant::now());
                }
            }
        }
    }
}

async fn send_line<W>(writer: &mut W, line: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

#[derive(Debug)]
enum Inbound {
    Ping(String),
    Welcome,
    AuthFailed(String),
    Reconnect,
    Chat(ChatMessage),
    Other,
}

/// Maps one raw line to what the session should do with it. Dispatches on
/// the parsed command only: chat text is viewer-controlled, and substring
/// checks on the raw line once let a viewer disconnect the bot by typing a
/// server NOTICE into chat.
fn classify(line: &str, own_login: &str) -> Inbound {
    let Some(parsed) = irc::parse(line) else {
        return Inbound::Other;
    };

    match parsed.command.as_str() {
        "PING" => Inbound::Ping(parsed.trailing().unwrap_or("tmi.twitch.tv").to_string()),
        "001" => Inbound::Welcome,
        "RECONNECT" => Inbound::Reconnect,
        "NOTICE" => {
            // Notices addressed to `*` are pre-login; only those can mean
            // our credentials were rejected.
            if parsed.params.first().map(String::as_str) != Some("*") {
                return Inbound::Other;
            }
            let text = parsed.trailing().unwrap_or_default();
            if text.contains("Login authentication failed") {
                Inbound::AuthFailed(
                    "Twitch rejected the bot's login. Sign in again, or check the username and token."
                        .to_string(),
                )
            } else if text.contains("Improperly formatted auth") {
                Inbound::AuthFailed(
                    "Twitch rejected the token format. Sign in again, or paste a token starting with oauth:."
                        .to_string(),
                )
            } else {
                Inbound::Other
            }
        }
        "PRIVMSG" => match privmsg_from(&parsed) {
            Some(message) if !message.login.eq_ignore_ascii_case(own_login) => {
                Inbound::Chat(message)
            }
            _ => Inbound::Other,
        },
        _ => Inbound::Other,
    }
}

/// Twitch allows an account that is not a moderator or the broadcaster in
/// the channel roughly 20 messages per 30 seconds; going over can get the
/// bot temporarily blocked from chat. A minimum gap also avoids tripping
/// spam detection with bursts.
struct ChatRateLimiter {
    max_in_window: usize,
    window: Duration,
    min_gap: Duration,
    sent: VecDeque<Instant>,
}

impl ChatRateLimiter {
    fn twitch_default() -> Self {
        Self {
            max_in_window: 20,
            window: Duration::from_secs(30),
            min_gap: Duration::from_millis(1100),
            sent: VecDeque::new(),
        }
    }

    /// Earliest moment the next message may be sent.
    fn next_ready(&mut self, now: Instant) -> Instant {
        while let Some(oldest) = self.sent.front() {
            if now.saturating_duration_since(*oldest) >= self.window {
                self.sent.pop_front();
            } else {
                break;
            }
        }

        let mut ready = now;
        if let Some(last) = self.sent.back() {
            ready = ready.max(*last + self.min_gap);
        }
        if self.sent.len() >= self.max_in_window {
            let gate = self.sent[self.sent.len() - self.max_in_window];
            ready = ready.max(gate + self.window);
        }
        ready
    }

    fn record(&mut self, at: Instant) {
        self.sent.push_back(at);
    }
}

fn format_privmsg(channel: &str, text: &str) -> String {
    let clean = sanitize_irc_text(text);
    let bounded: String = clean.chars().take(MAX_CHAT_CHARS).collect();
    format!("PRIVMSG #{channel} :{bounded}\r\n")
}

/// Strips line breaks so text echoed into chat (song titles, queries) can
/// never terminate the IRC line and smuggle in a second command.
fn sanitize_irc_text(text: &str) -> String {
    text.chars()
        .map(|ch| if ch == '\r' || ch == '\n' { ' ' } else { ch })
        .collect()
}

// --- Worker ---------------------------------------------------------------------

async fn event_worker(
    context: Arc<AppContext>,
    mut events: mpsc::Receiver<SessionEvent>,
    outbound: mpsc::UnboundedSender<Outbound>,
    channel: String,
) {
    while let Some(event) = events.recv().await {
        match event {
            SessionEvent::Welcome => {
                let request_command = context.current_settings().await.twitch.request_command;
                context
                    .update_bot_status(
                        BotConnectionState::Connected,
                        "Connected",
                        format!("Listening in #{channel}."),
                        Some(channel.clone()),
                    )
                    .await;
                context
                    .add_log(
                        LogLevel::Info,
                        format!(
                            "Twitch bot connected to #{channel} and is listening for {request_command} / !remove / !song / !queue / !skip."
                        ),
                    )
                    .await;
            }
            SessionEvent::Chat(message) => {
                if let Some(reply) = handle_chat_command(&context, &message).await {
                    let _ = outbound.send(Outbound::chat(format!("@{} {}", message.login, reply)));
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub login: String,
    pub display_name: String,
    pub message: String,
    pub is_mod_or_broadcaster: bool,
}

/// Runs one chat command and returns the reply to post, if any.
async fn handle_chat_command(context: &Arc<AppContext>, message: &ChatMessage) -> Option<String> {
    let text = message.message.trim();
    if !text.starts_with('!') {
        return None;
    }

    let settings = context.current_settings().await;
    let mut parts = text.split_whitespace();
    let command = parts.next().unwrap_or_default().to_lowercase();
    let args = parts.collect::<Vec<_>>().join(" ");
    let request_command = settings.twitch.request_command.trim().to_lowercase();

    // The configured request command always wins, even if a streamer points
    // it at one of the built-in names below (e.g. sets !song as their request
    // command) — check it first so it never gets shadowed.
    if !request_command.is_empty() && command == request_command {
        let is_privileged =
            message.is_mod_or_broadcaster && settings.request_limits.mods_bypass_limits;
        let result = context
            .process_request(&message.display_name, &args, is_privileged, "twitch")
            .await;
        context
            .add_log(
                if result.ok { LogLevel::Info } else { LogLevel::Warn },
                format!(
                    "Twitch request from @{}: {} ({})",
                    message.display_name,
                    if args.trim().is_empty() { "<empty request>" } else { args.trim() },
                    result.message
                ),
            )
            .await;
        return Some(result.message);
    }

    match command.as_str() {
        "!remove" => {
            let result = context
                .remove_latest_request_by_user(&message.display_name)
                .await;
            context
                .add_log(
                    if result.ok { LogLevel::Info } else { LogLevel::Warn },
                    format!("Twitch !remove from @{}: {}", message.display_name, result.message),
                )
                .await;
            Some(result.message)
        }
        "!song" => {
            let reply = format_song_reply(&context.current_probe().await);
            context
                .add_log(
                    LogLevel::Info,
                    format!("Twitch !song from @{}: {}", message.display_name, reply),
                )
                .await;
            Some(reply)
        }
        "!queue" => {
            let queue = context.current_queue().await;
            let reply = queue_engine::format_queue_reply(&queue, &message.display_name);
            context
                .add_log(
                    LogLevel::Info,
                    format!("Twitch !queue from @{}: {}", message.display_name, reply),
                )
                .await;
            Some(reply)
        }
        "!skip" => {
            if !message.is_mod_or_broadcaster {
                context
                    .add_log(
                        LogLevel::Warn,
                        format!(
                            "Twitch !skip denied for @{} (not a mod/broadcaster).",
                            message.display_name
                        ),
                    )
                    .await;
                return Some("Only mods can skip.".to_string());
            }
            let (ok, reply) = match context
                .player_bridge
                .run_command(&context.handle, "skip", None)
                .await
            {
                Ok(_) => (true, "Skipped.".to_string()),
                Err(error) => (false, error.to_string()),
            };
            context
                .add_log(
                    if ok { LogLevel::Info } else { LogLevel::Warn },
                    format!("Twitch !skip from @{}: {}", message.display_name, reply),
                )
                .await;
            Some(reply)
        }
        _ => None,
    }
}

/// Formats the reply for the `!song` chat command from the embedded
/// player's probe snapshot. Pure so it can be unit tested without an
/// `AppContext`.
fn format_song_reply(probe: &ProbeSnapshot) -> String {
    let title = probe.title.trim();
    let is_playing_or_paused =
        probe.status.eq_ignore_ascii_case("playing") || probe.status.eq_ignore_ascii_case("paused");

    if title.is_empty() || !is_playing_or_paused {
        return "Nothing is playing right now.".to_string();
    }

    let artist = probe.artist.trim();
    if artist.is_empty() {
        format!("Now playing: {title}")
    } else {
        format!("Now playing: {title} — {artist}")
    }
}

fn privmsg_from(parsed: &irc::IrcMessage) -> Option<ChatMessage> {
    if parsed.command != "PRIVMSG" || parsed.params.len() < 2 {
        return None;
    }

    let message = parsed.trailing()?.to_string();
    let login = parsed.nick()?.to_string();
    let display_name = parsed
        .tag("display-name")
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| login.clone());
    let is_mod_or_broadcaster = parsed.tag("mod") == Some("1")
        || parsed
            .tag("badges")
            .map(|badges| badges.split(',').any(|badge| badge == "broadcaster/1"))
            .unwrap_or(false);

    Some(ChatMessage {
        login,
        display_name,
        message,
        is_mod_or_broadcaster,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const FAST: Timing = Timing {
        keepalive_every: Duration::from_millis(20),
        stale_after: Duration::from_millis(60),
    };

    fn identity() -> ChatIdentity {
        ChatIdentity {
            login: "bot".to_string(),
            password: "oauth:secret".to_string(),
            refreshable: false,
        }
    }

    /// Runs a session against a scripted in-memory "server" and returns the
    /// outcome plus everything the client wrote.
    async fn run_scripted(script: &'static [u8]) -> (SessionOutcome, String, Vec<SessionEvent>) {
        let (client, mut server) = tokio::io::duplex(16 * 1024);
        let (_outbound_tx, mut outbound_rx) = mpsc::unbounded_channel();
        let (event_tx, mut event_rx) = mpsc::channel(16);

        let session = tokio::spawn(async move {
            session_loop(client, &identity(), "chan", &mut outbound_rx, &event_tx, FAST).await
        });

        server.write_all(script).await.unwrap();
        let outcome = session.await.unwrap();

        let mut written = String::new();
        server.read_to_string(&mut written).await.unwrap();
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        (outcome, written, events)
    }

    #[tokio::test]
    async fn logs_in_answers_ping_and_honours_reconnect() {
        let (outcome, written, _) =
            run_scripted(b":tmi.twitch.tv 001 bot :Welcome\r\nPING :tmi.twitch.tv\r\n:tmi.twitch.tv RECONNECT\r\n").await;

        assert!(matches!(outcome.end, SessionEnd::Reconnect));
        assert!(outcome.welcomed);
        assert!(written.contains("PASS oauth:secret\r\n"));
        assert!(written.contains("NICK bot\r\n"));
        assert!(written.contains("JOIN #chan\r\n"));
        assert!(written.contains("PONG :tmi.twitch.tv\r\n"));
    }

    // Regression: a viewer typing an auth-failure NOTICE must stay a chat
    // message and must not end the session.
    #[tokio::test]
    async fn chat_impersonating_auth_failure_does_not_disconnect() {
        let (outcome, _, events) = run_scripted(
            b"@display-name=Troll :troll!troll@troll.tmi.twitch.tv PRIVMSG #chan :NOTICE * :Login authentication failed\r\n:tmi.twitch.tv RECONNECT\r\n",
        )
        .await;

        assert!(matches!(outcome.end, SessionEnd::Reconnect));
        assert!(events.iter().any(|event| matches!(event, SessionEvent::Chat(message) if message.login == "troll")));
    }

    #[tokio::test]
    async fn real_auth_failure_ends_session() {
        let (outcome, _, _) = run_scripted(b":tmi.twitch.tv NOTICE * :Login authentication failed\r\n").await;
        assert!(matches!(outcome.end, SessionEnd::AuthFailed(_)));
    }

    // A half-dead socket reports no error; silence must still end the session
    // so the supervisor can reconnect.
    #[tokio::test]
    async fn silent_connection_is_detected_as_stale() {
        let (client, _server) = tokio::io::duplex(1024);
        let (_outbound_tx, mut outbound_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = mpsc::channel(4);

        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            session_loop(client, &identity(), "chan", &mut outbound_rx, &event_tx, FAST),
        )
        .await
        .expect("stale connection must end the session");
        assert!(matches!(outcome.end, SessionEnd::Stale));
    }

    #[tokio::test]
    async fn own_messages_are_ignored() {
        let (_, _, events) = run_scripted(
            b":bot!bot@bot.tmi.twitch.tv PRIVMSG #chan :!sr echo\r\n:tmi.twitch.tv RECONNECT\r\n",
        )
        .await;
        assert!(events.is_empty());
    }

    #[test]
    fn rate_limiter_enforces_gap_and_window() {
        let mut limiter = ChatRateLimiter::twitch_default();
        let start = Instant::now();

        assert_eq!(limiter.next_ready(start), start);
        limiter.record(start);
        assert_eq!(limiter.next_ready(start), start + Duration::from_millis(1100));

        // Fill the window: the 21st message must wait for the first to age out.
        let mut at = start;
        for _ in 1..20 {
            at += Duration::from_millis(1100);
            limiter.record(at);
        }
        let ready = limiter.next_ready(at);
        assert_eq!(ready, start + Duration::from_secs(30));
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_delay(1), Duration::from_secs(2));
        assert_eq!(backoff_delay(3), Duration::from_secs(8));
        assert_eq!(backoff_delay(20), Duration::from_secs(60));
    }

    #[test]
    fn privmsg_is_sanitized_and_bounded() {
        let line = format_privmsg("chan", "evil\r\nPRIVMSG #other :spam");
        assert_eq!(line.matches("\r\n").count(), 1, "only the terminator may be a line break");
        let long = format_privmsg("chan", &"x".repeat(900));
        assert!(long.len() < 520);
    }

    #[test]
    fn format_song_reply_reports_playing_track_with_artist() {
        let probe = ProbeSnapshot {
            status: "Playing".to_string(),
            title: "Human Nature".to_string(),
            artist: "Michael Jackson".to_string(),
            ..ProbeSnapshot::default()
        };
        assert_eq!(format_song_reply(&probe), "Now playing: Human Nature — Michael Jackson");
    }

    #[test]
    fn format_song_reply_accepts_paused_status() {
        let probe = ProbeSnapshot {
            status: "Paused".to_string(),
            title: "Freefall".to_string(),
            artist: "Durand Bernarr".to_string(),
            ..ProbeSnapshot::default()
        };
        assert_eq!(format_song_reply(&probe), "Now playing: Freefall — Durand Bernarr");
    }

    #[test]
    fn format_song_reply_reports_nothing_playing_when_stopped() {
        let probe = ProbeSnapshot {
            status: "Stopped".to_string(),
            title: "Human Nature".to_string(),
            ..ProbeSnapshot::default()
        };
        assert_eq!(format_song_reply(&probe), "Nothing is playing right now.");
    }

    #[test]
    fn parses_privmsg_with_mod_badge() {
        let parsed = irc::parse("@badges=broadcaster/1;display-name=Streamer;mod=0 :streamer!streamer@streamer.tmi.twitch.tv PRIVMSG #streamer :!skip").unwrap();
        let message = privmsg_from(&parsed).expect("should parse");
        assert_eq!(message.display_name, "Streamer");
        assert_eq!(message.message, "!skip");
        assert!(message.is_mod_or_broadcaster);
    }

    #[test]
    fn broadcaster_badge_match_is_exact() {
        let parsed = irc::parse("@badges=notbroadcaster/1;mod=0 :v!v@v PRIVMSG #c :hi").unwrap();
        assert!(!privmsg_from(&parsed).unwrap().is_mod_or_broadcaster);
    }
}
