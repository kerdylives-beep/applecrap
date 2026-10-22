//! "Now playing" posts in chat, one per song. Off unless the streamer turns
//! it on. Listen together (listener mode) reads these posts to follow the
//! stream's music in each viewer's own Apple Music, so the format matters:
//! the Apple Music link goes last, as `https://music.apple.com/{sf}/song/{id}`.

use super::*;

/// Twitch drops chat messages over 500 characters.
const MAX_CHAT_CHARS: usize = 500;

impl AppContext {
    /// Called on every probe cycle. Announces a song once, when it's playing
    /// and differs from the last one announced.
    pub(super) async fn announce_if_new_song(&self, probe: &ProbeSnapshot, top_item: Option<&QueueItem>) {
        if !probe.status.eq_ignore_ascii_case("playing") || probe.title.trim().is_empty() {
            return;
        }
        let key = song_key(probe);
        let requested_by = {
            let mut runtime = self.runtime.write().await;
            if runtime.last_announced.as_deref() == Some(key.as_str()) {
                return;
            }
            // Remembered even with announcements off, so turning them on
            // mid-song doesn't announce a song that's halfway through.
            runtime.last_announced = Some(key);
            // A request that just started is still the queue's first item;
            // one confirmed earlier is kept aside for the overlay.
            if probe.matched {
                top_item.map(|item| item.requested_by.clone())
            } else {
                overlay::current_request(&runtime).map(|request| request.requested_by)
            }
        };

        let settings = self.current_settings().await;
        if !settings.twitch.announce_songs {
            return;
        }
        let text = announcement(
            probe,
            &settings.apple_music.storefront,
            requested_by.as_deref().filter(|name| !name.trim().is_empty()),
        );
        self.send_chat(text).await;
    }
}

/// What makes two probes "the same song".
fn song_key(probe: &ProbeSnapshot) -> String {
    match probe.catalog_id.as_deref() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("{}\u{1f}{}", probe.title.trim(), probe.artist.trim()),
    }
}

/// "🎵 Now playing: Title — Artist · requested by @name · <link>". The link
/// stays whole; the title is shortened if the message would be too long.
fn announcement(probe: &ProbeSnapshot, storefront: &str, requested_by: Option<&str>) -> String {
    let artist = probe.artist.trim();
    let mut tail = String::new();
    if !artist.is_empty() {
        tail.push_str(" \u{2014} ");
        tail.push_str(artist);
    }
    if let Some(name) = requested_by {
        tail.push_str(" \u{00b7} requested by @");
        tail.push_str(name.trim_start_matches('@'));
    }
    if let Some(id) = probe.catalog_id.as_deref().filter(|id| !id.is_empty()) {
        let storefront = match storefront.trim() {
            "" => "us".to_string(),
            other => other.to_lowercase(),
        };
        tail.push_str(&format!(" \u{00b7} https://music.apple.com/{storefront}/song/{id}"));
    }

    let head = "\u{1f3b5} Now playing: ";
    let room = MAX_CHAT_CHARS
        .saturating_sub(head.chars().count() + tail.chars().count())
        .max(1);
    let title = probe.title.trim();
    let title = if title.chars().count() > room {
        let mut cut: String = title.chars().take(room.saturating_sub(1)).collect();
        cut.push('\u{2026}');
        cut
    } else {
        title.to_string()
    };
    let mut text = format!("{head}{title}{tail}");
    if text.chars().count() > MAX_CHAT_CHARS {
        text = text.chars().take(MAX_CHAT_CHARS).collect();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(title: &str, artist: &str, id: Option<&str>) -> ProbeSnapshot {
        ProbeSnapshot {
            status: "Playing".to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            catalog_id: id.map(str::to_string),
            ..ProbeSnapshot::default()
        }
    }

    #[test]
    fn announcement_names_the_song_requester_and_link() {
        let probe = playing("Push Up", "Sullivan King, T-Pain & Zoey808", Some("6774717255"));
        assert_eq!(
            announcement(&probe, "us", Some("keykeykeeks")),
            "\u{1f3b5} Now playing: Push Up \u{2014} Sullivan King, T-Pain & Zoey808 \u{00b7} requested by @keykeykeeks \u{00b7} https://music.apple.com/us/song/6774717255"
        );
        assert_eq!(
            announcement(&probe, "", None),
            "\u{1f3b5} Now playing: Push Up \u{2014} Sullivan King, T-Pain & Zoey808 \u{00b7} https://music.apple.com/us/song/6774717255"
        );
    }

    #[test]
    fn announcement_without_a_catalog_id_has_no_link() {
        let probe = playing("Home Recording", "Me", None);
        assert_eq!(announcement(&probe, "us", None), "\u{1f3b5} Now playing: Home Recording \u{2014} Me");
    }

    #[test]
    fn announcement_fits_in_one_chat_message_and_keeps_the_link() {
        let probe = playing(&"A".repeat(800), "Artist", Some("123"));
        let text = announcement(&probe, "gb", None);
        assert!(text.chars().count() <= MAX_CHAT_CHARS);
        assert!(text.ends_with("https://music.apple.com/gb/song/123"));
        assert!(text.contains('\u{2026}'));
    }

    #[test]
    fn song_key_prefers_the_catalog_id() {
        assert_eq!(song_key(&playing("X", "Y", Some("42"))), "42");
        assert_ne!(song_key(&playing("X", "Y", None)), song_key(&playing("X", "Z", None)));
    }
}
