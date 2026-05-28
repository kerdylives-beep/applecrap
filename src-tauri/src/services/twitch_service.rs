use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::mpsc,
};
use tokio_native_tls::{native_tls, TlsConnector};

use crate::{
    app::AppContext,
    models::{normalize_twitch_oauth_token, AppSettings, BotConnectionState, LogLevel},
};

pub async fn connect(context: Arc<AppContext>) -> Result<()> {
    let settings = context.current_settings().await;
    let channel = settings
        .twitch
        .channel
        .trim()
        .trim_start_matches('#')
        .to_string();
    let username = settings.twitch.bot_username.trim().to_string();
    let oauth_token = normalize_twitch_oauth_token(&settings.twitch.oauth_token);
    let request_command = settings.twitch.request_command.trim().to_string();

    if channel.is_empty() || username.is_empty() || oauth_token.is_empty() {
        return Err(anyhow!(
            "Twitch channel, bot username, and OAuth token are required."
        ));
    }

    context.abort_twitch_connection().await;
    context
        .update_bot_status(
            BotConnectionState::Connecting,
            "Connecting",
            format!("Connecting to Twitch IRC for #{}...", channel),
            Some(channel.clone()),
        )
        .await;
    context
        .add_log(
            LogLevel::Info,
            format!(
                "Connecting Twitch bot as @{} to #{} (listening for {} and !remove).",
                username, channel, request_command
            ),
        )
        .await;

    let (writer_tx, writer_rx) = mpsc::unbounded_channel::<String>();
    let task_context = Arc::clone(&context);
    let task_channel = channel.clone();
    let task_writer_tx = writer_tx.clone();
    let task = tauri::async_runtime::spawn(async move {
        match run_client(
            Arc::clone(&task_context),
            settings,
            writer_rx,
            task_writer_tx.clone(),
        )
        .await
        {
            Ok(()) => {
                task_context
                    .update_bot_status(
                        BotConnectionState::Disconnected,
                        "Disconnected",
                        "Bot is offline.",
                        Some(task_channel.clone()),
                    )
                    .await;
            }
            Err(error) => {
                task_context
                    .update_bot_status(
                        BotConnectionState::Error,
                        "Connection error",
                        error.to_string(),
                        Some(task_channel.clone()),
                    )
                    .await;
                task_context
                    .add_log(LogLevel::Error, format!("Twitch bot disconnected: {error}"))
                    .await;
            }
        }

        task_context.clear_twitch_connection().await;
    });

    context.register_twitch_connection(writer_tx, task).await;
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

async fn run_client(
    context: Arc<AppContext>,
    settings: AppSettings,
    mut writer_rx: mpsc::UnboundedReceiver<String>,
    writer_tx: mpsc::UnboundedSender<String>,
) -> Result<()> {
    let channel = settings
        .twitch
        .channel
        .trim()
        .trim_start_matches('#')
        .to_string();
    let username = settings.twitch.bot_username.trim().to_string();
    let oauth_token = normalize_twitch_oauth_token(&settings.twitch.oauth_token);

    let stream = TcpStream::connect(("irc.chat.twitch.tv", 6697)).await?;
    let connector = TlsConnector::from(native_tls::TlsConnector::builder().build()?);
    let tls_stream = connector.connect("irc.chat.twitch.tv", stream).await?;
    let (read_half, mut write_half) = tokio::io::split(tls_stream);
    let mut lines = BufReader::new(read_half).lines();

    send_raw(
        &mut write_half,
        "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n",
    )
    .await?;
    send_raw(&mut write_half, &format!("PASS {}\r\n", oauth_token)).await?;
    send_raw(&mut write_half, &format!("NICK {}\r\n", username)).await?;
    send_raw(&mut write_half, &format!("JOIN #{}\r\n", channel)).await?;

    loop {
        tokio::select! {
            maybe_outbound = writer_rx.recv() => {
                match maybe_outbound {
                    Some(line) => send_raw(&mut write_half, &line).await?,
                    None => break,
                }
            }
            maybe_line = lines.next_line() => {
                match maybe_line? {
                    Some(line) => handle_irc_line(&context, &channel, &username, &writer_tx, &line).await?,
                    None => break,
                }
            }
        }
    }

    Ok(())
}

async fn handle_irc_line(
    context: &Arc<AppContext>,
    configured_channel: &str,
    bot_username: &str,
    writer_tx: &mpsc::UnboundedSender<String>,
    line: &str,
) -> Result<()> {
    if let Some(ping_payload) = line.strip_prefix("PING :") {
        let _ = writer_tx.send(format!("PONG :{}\r\n", ping_payload));
        return Ok(());
    }

    if line.contains(" 001 ") {
        let request_command = context.current_settings().await.twitch.request_command;
        context
            .update_bot_status(
                BotConnectionState::Connected,
                "Connected",
                format!("Listening in #{}.", configured_channel),
                Some(configured_channel.to_string()),
            )
            .await;
        context
            .add_log(
                LogLevel::Info,
                format!(
                    "Twitch bot connected to #{} and is listening for {} / !remove.",
                    configured_channel, request_command
                ),
            )
            .await;
        return Ok(());
    }

    if line.contains("NOTICE * :Login authentication failed") {
        anyhow::bail!("Twitch login failed. Check the bot username and OAuth token.");
    }

    if line.contains("NOTICE * :Improperly formatted auth") {
        anyhow::bail!(
            "Twitch rejected the auth format. The token should come from a bot account and start with oauth:."
        );
    }

    let Some(message) = parse_privmsg(line) else {
        return Ok(());
    };

    if message.login.eq_ignore_ascii_case(bot_username) {
        return Ok(());
    }

    let request_command = context.current_settings().await.twitch.request_command;
    let normalized_message = message.message.trim();
    let mut parts = normalized_message.split_whitespace();
    let command = parts.next().unwrap_or_default().to_lowercase();
    let args = parts.collect::<Vec<_>>().join(" ");

    if command == "!remove" {
        let result = context
            .remove_latest_request_by_user(&message.display_name)
            .await;
        context
            .add_log(
                if result.ok {
                    LogLevel::Info
                } else {
                    LogLevel::Warn
                },
                format!(
                    "Twitch !remove from @{}: {}",
                    message.display_name, result.message
                ),
            )
            .await;
        let _ = writer_tx.send(format!(
            "PRIVMSG #{} :@{} {}\r\n",
            message.channel, message.login, result.message
        ));
        return Ok(());
    }

    if command != request_command.to_lowercase() {
        if command.starts_with('!') {
            context
                .add_log(
                    LogLevel::Debug,
                    format!(
                        "Ignored Twitch command {} from @{}. Live request command is {}.",
                        command, message.display_name, request_command
                    ),
                )
                .await;
        }
        return Ok(());
    }

    let is_privileged = message.is_mod_or_broadcaster
        && context
            .current_settings()
            .await
            .request_limits
            .mods_bypass_limits;
    let result = context
        .process_request(&message.display_name, &args, is_privileged, "twitch")
        .await;
    context
        .add_log(
            if result.ok {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            format!(
                "Twitch request from @{}: {} ({})",
                message.display_name,
                if args.trim().is_empty() {
                    "<empty request>"
                } else {
                    args.trim()
                },
                result.message
            ),
        )
        .await;
    let _ = writer_tx.send(format!(
        "PRIVMSG #{} :@{} {}\r\n",
        message.channel, message.login, result.message
    ));

    Ok(())
}

async fn send_raw<W>(writer: &mut W, line: &str) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

struct ParsedMessage {
    channel: String,
    login: String,
    display_name: String,
    message: String,
    is_mod_or_broadcaster: bool,
}

fn parse_privmsg(line: &str) -> Option<ParsedMessage> {
    let (tags_part, rest) = if let Some(stripped) = line.strip_prefix('@') {
        let split_index = stripped.find(' ')?;
        (&stripped[..split_index], &stripped[split_index + 1..])
    } else {
        ("", line)
    };

    if !rest.contains(" PRIVMSG ") {
        return None;
    }

    let trailing_index = rest.find(" :")?;
    let leading = &rest[..trailing_index];
    let message = &rest[trailing_index + 2..];
    let channel = leading
        .split_whitespace()
        .nth(2)?
        .trim_start_matches('#')
        .to_string();
    let login = rest
        .split_whitespace()
        .next()
        .unwrap_or(":viewer")
        .trim_start_matches(':')
        .split('!')
        .next()
        .unwrap_or("viewer")
        .to_string();
    let tags = parse_tags(tags_part);
    let display_name = tags
        .get("display-name")
        .cloned()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| login.clone());
    let is_mod_or_broadcaster = tags.get("mod").map(|value| value == "1").unwrap_or(false)
        || tags
            .get("badges")
            .map(|value| value.contains("broadcaster/1"))
            .unwrap_or(false);

    Some(ParsedMessage {
        channel,
        login,
        display_name,
        message: message.to_string(),
        is_mod_or_broadcaster,
    })
}

fn parse_tags(input: &str) -> HashMap<String, String> {
    input
        .split(';')
        .filter_map(|entry| entry.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}
