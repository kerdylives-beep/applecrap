//! Minimal IRCv3 line parser for Twitch chat.
//!
//! Lines have the shape `[@tags ][:prefix ]COMMAND[ params][ :trailing]`.
//! Everything that reacts to chat must branch on the parsed `command`, never
//! on substrings of the raw line: chat text is attacker-controlled, and a
//! viewer typing a server message into chat must stay a PRIVMSG.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrcMessage {
    pub tags: HashMap<String, String>,
    pub prefix: Option<String>,
    /// Upper-cased command or numeric, e.g. `PRIVMSG`, `NOTICE`, `001`.
    pub command: String,
    /// Middle params followed by the trailing param, if any.
    pub params: Vec<String>,
}

impl IrcMessage {
    /// The trailing parameter (message text for PRIVMSG/NOTICE).
    pub fn trailing(&self) -> Option<&str> {
        self.params.last().map(String::as_str)
    }

    /// Nick portion of the prefix (`nick!user@host` -> `nick`).
    pub fn nick(&self) -> Option<&str> {
        self.prefix
            .as_deref()
            .map(|prefix| prefix.split('!').next().unwrap_or(prefix))
    }

    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).map(String::as_str)
    }
}

pub fn parse(line: &str) -> Option<IrcMessage> {
    let mut rest = line.trim_end_matches(['\r', '\n']);
    let mut tags = HashMap::new();

    if let Some(stripped) = rest.strip_prefix('@') {
        let (raw_tags, after) = stripped.split_once(' ')?;
        tags = parse_tags(raw_tags);
        rest = after.trim_start_matches(' ');
    }

    let mut prefix = None;
    if let Some(stripped) = rest.strip_prefix(':') {
        let (raw_prefix, after) = stripped.split_once(' ')?;
        prefix = Some(raw_prefix.to_string());
        rest = after.trim_start_matches(' ');
    }

    let (command, mut remaining) = match rest.split_once(' ') {
        Some((command, after)) => (command, after),
        None => (rest, ""),
    };
    if command.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    loop {
        let current = remaining.trim_start_matches(' ');
        if current.is_empty() {
            break;
        }
        if let Some(trailing) = current.strip_prefix(':') {
            params.push(trailing.to_string());
            break;
        }
        match current.split_once(' ') {
            Some((param, after)) => {
                params.push(param.to_string());
                remaining = after;
            }
            None => {
                params.push(current.to_string());
                break;
            }
        }
    }

    Some(IrcMessage {
        tags,
        prefix,
        command: command.to_ascii_uppercase(),
        params,
    })
}

fn parse_tags(raw: &str) -> HashMap<String, String> {
    raw.split(';')
        .filter(|entry| !entry.is_empty())
        .map(|entry| match entry.split_once('=') {
            Some((key, value)) => (key.to_string(), unescape_tag_value(value)),
            None => (entry.to_string(), String::new()),
        })
        .collect()
}

/// IRCv3 tag value unescaping (`\s` space, `\:` semicolon, `\\` backslash).
fn unescape_tag_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('\\') => out.push('\\'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tagged_privmsg() {
        let msg = parse("@badges=broadcaster/1;display-name=Streamer;mod=0 :streamer!streamer@streamer.tmi.twitch.tv PRIVMSG #streamer :!sr human nature").unwrap();
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.params, vec!["#streamer", "!sr human nature"]);
        assert_eq!(msg.nick(), Some("streamer"));
        assert_eq!(msg.tag("display-name"), Some("Streamer"));
    }

    // Regression: chat text impersonating a server NOTICE used to disconnect
    // the bot, because auth failure was detected by substring on the raw line.
    #[test]
    fn chat_text_impersonating_notice_stays_privmsg() {
        let msg = parse("@display-name=Troll :troll!troll@troll.tmi.twitch.tv PRIVMSG #chan :NOTICE * :Login authentication failed").unwrap();
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.trailing(), Some("NOTICE * :Login authentication failed"));
    }

    // Regression: " 001 " in chat was mistaken for the server welcome.
    #[test]
    fn chat_text_containing_001_stays_privmsg() {
        let msg = parse(":viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #chan :agent 001 reporting").unwrap();
        assert_eq!(msg.command, "PRIVMSG");
    }

    #[test]
    fn parses_real_auth_failure_notice() {
        let msg = parse(":tmi.twitch.tv NOTICE * :Login authentication failed").unwrap();
        assert_eq!(msg.command, "NOTICE");
        assert_eq!(msg.params, vec!["*", "Login authentication failed"]);
    }

    #[test]
    fn parses_ping_reconnect_and_welcome() {
        let ping = parse("PING :tmi.twitch.tv").unwrap();
        assert_eq!(ping.command, "PING");
        assert_eq!(ping.trailing(), Some("tmi.twitch.tv"));

        let reconnect = parse(":tmi.twitch.tv RECONNECT").unwrap();
        assert_eq!(reconnect.command, "RECONNECT");
        assert!(reconnect.params.is_empty());

        let welcome = parse(":tmi.twitch.tv 001 bot :Welcome, GLHF!\r\n").unwrap();
        assert_eq!(welcome.command, "001");
        assert_eq!(welcome.params, vec!["bot", "Welcome, GLHF!"]);
    }

    #[test]
    fn unescapes_tag_values() {
        let msg = parse("@display-name=Cool\\sName;note=a\\:b\\\\c :x!x@x PRIVMSG #c :hi").unwrap();
        assert_eq!(msg.tag("display-name"), Some("Cool Name"));
        assert_eq!(msg.tag("note"), Some("a;b\\c"));
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(parse("").is_none());
        assert!(parse("@onlytags").is_none());
        assert!(parse(":onlyprefix").is_none());
    }
}
