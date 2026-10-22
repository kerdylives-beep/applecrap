//! Listen together: reading the "Now playing" posts a streamer's AppleCrap
//! puts in chat (see `app/announce.rs` for the format).

const HEAD: &str = "\u{1f3b5} Now playing: ";
const LINK: &str = "https://music.apple.com/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlayingPost {
    pub catalog_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub requested_by: Option<String>,
}

/// Reads one chat message. `None` unless it is a "Now playing" post.
pub fn parse_now_playing(text: &str) -> Option<NowPlayingPost> {
    let body = text.trim().strip_prefix(HEAD)?;

    let (body, catalog_id) = match body.rfind(LINK) {
        Some(at) => (
            body[..at].trim_end().trim_end_matches('\u{00b7}').trim_end(),
            song_id(&body[at + LINK.len()..]),
        ),
        None => (body, None),
    };
    let (body, requested_by) = match body.rfind(" \u{00b7} requested by @") {
        Some(at) => (
            &body[..at],
            Some(body[at..].trim_start_matches(" \u{00b7} requested by @").trim().to_string())
                .filter(|name| !name.is_empty()),
        ),
        None => (body, None),
    };
    let (title, artist) = match body.rfind(" \u{2014} ") {
        Some(at) => (&body[..at], &body[at + " \u{2014} ".len()..]),
        None => (body, ""),
    };

    Some(NowPlayingPost {
        catalog_id,
        title: title.trim().to_string(),
        artist: artist.trim().to_string(),
        requested_by,
    })
}

/// The id from `{storefront}/song/{id}`, digits only.
fn song_id(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let storefront = parts.next()?;
    if storefront.len() != 2 || parts.next()? != "song" {
        return None;
    }
    let id: String = parts
        .next()?
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_full_post() {
        let post = parse_now_playing(
            "\u{1f3b5} Now playing: Buy U a Drank \u{2014} T-Pain \u{00b7} requested by @viewer \u{00b7} https://music.apple.com/us/song/1440841363",
        )
        .unwrap();
        assert_eq!(post.catalog_id.as_deref(), Some("1440841363"));
        assert_eq!(post.title, "Buy U a Drank");
        assert_eq!(post.artist, "T-Pain");
        assert_eq!(post.requested_by.as_deref(), Some("viewer"));
    }

    #[test]
    fn reads_a_post_without_requester_or_link() {
        let post = parse_now_playing("\u{1f3b5} Now playing: Song \u{2014} Band").unwrap();
        assert_eq!(post.catalog_id, None);
        assert_eq!(post.title, "Song");
        assert_eq!(post.artist, "Band");
        assert_eq!(post.requested_by, None);
    }

    #[test]
    fn keeps_dashes_inside_the_title() {
        let post = parse_now_playing(
            "\u{1f3b5} Now playing: A \u{2014} B (Remix) \u{2014} Artist \u{00b7} https://music.apple.com/gb/song/42",
        )
        .unwrap();
        assert_eq!(post.title, "A \u{2014} B (Remix)");
        assert_eq!(post.artist, "Artist");
        assert_eq!(post.catalog_id.as_deref(), Some("42"));
    }

    #[test]
    fn ignores_other_messages_and_odd_links() {
        assert_eq!(parse_now_playing("Now playing: something"), None);
        assert_eq!(parse_now_playing("!song"), None);
        let post = parse_now_playing(
            "\u{1f3b5} Now playing: X \u{2014} Y \u{00b7} https://music.apple.com/us/album/abc",
        )
        .unwrap();
        assert_eq!(post.catalog_id, None);
    }
}
