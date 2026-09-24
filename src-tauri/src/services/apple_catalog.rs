use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;

use crate::{
    models::{AppleMusicSettings, TrackMatch},
    services::queue_engine::{
        extract_apple_music_track_id, is_url, normalize_text, strip_featuring, token_overlap,
    },
};

const ITUNES_SEARCH_BASE: &str = "https://itunes.apple.com/search";
const ITUNES_LOOKUP_BASE: &str = "https://itunes.apple.com/lookup";

#[derive(Clone)]
pub struct AppleCatalog {
    client: Client,
}

impl AppleCatalog {
    /// Takes the app's shared HTTP client, which carries the timeouts: a
    /// lookup with none could hang forever and stall request handling.
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn search_top_track(
        &self,
        query: &str,
        settings: &AppleMusicSettings,
    ) -> Result<Option<TrackMatch>> {
        if is_url(query) {
            let track_id = match extract_apple_music_track_id(query) {
                Some(track_id) => track_id,
                None => return Ok(None),
            };

            return self.lookup_track_by_id(&track_id, settings).await;
        }

        let results = self.search_tracks(query, settings).await?;
        Ok(results.into_iter().next())
    }

    pub async fn lookup_track_by_id(
        &self,
        track_id: &str,
        settings: &AppleMusicSettings,
    ) -> Result<Option<TrackMatch>> {
        let mut url = reqwest::Url::parse(ITUNES_LOOKUP_BASE)?;
        url.query_pairs_mut()
            .append_pair("id", track_id)
            .append_pair("country", &settings.storefront.to_uppercase())
            .append_pair("entity", "song");

        let response = self.client.get(url).send().await?;
        let response = response
            .error_for_status()
            .context("iTunes Lookup API request failed")?;
        let payload: ItunesResponse = response.json().await?;

        Ok(payload
            .results
            .into_iter()
            .find(|song| {
                song.wrapper_type.as_deref() == Some("track")
                    && song.kind.as_deref() == Some("song")
            })
            .and_then(|song| song.into_track_match(&settings.storefront)))
    }

    pub async fn search_tracks(
        &self,
        query: &str,
        settings: &AppleMusicSettings,
    ) -> Result<Vec<TrackMatch>> {
        let mut matches = self.fetch_itunes_tracks(query, settings).await?;
        if matches.is_empty() {
            let mut web = self.fetch_web_search_tracks(query, settings).await?;
            if let Some(first) = web.first_mut() {
                self.fill_in_details(first, settings).await;
            }
            return Ok(web);
        }

        // The search API sometimes leaves the original out entirely (seen
        // with explicit tracks) and returns only covers of it. When the
        // request names an artist and the best pick isn't theirs, ask the
        // Apple Music site search, which does list the original.
        if !is_confident(query, &matches) {
            let web = self
                .fetch_web_search_tracks(query, settings)
                .await
                .unwrap_or_default();
            if let Some(mut original) = web
                .into_iter()
                .find(|track| artist_named_in(query, &track.artist_name))
            {
                self.fill_in_details(&mut original, settings).await;
                matches.retain(|track| track.id != original.id);
                matches.insert(0, original);
            }
        }

        Ok(matches)
    }

    /// Site search results carry no album, artwork or length; the lookup
    /// API has them.
    async fn fill_in_details(&self, track: &mut TrackMatch, settings: &AppleMusicSettings) {
        if let Ok(Some(full)) = self.lookup_track_by_id(&track.id, settings).await {
            *track = full;
        }
    }

    async fn fetch_itunes_tracks(
        &self,
        query: &str,
        settings: &AppleMusicSettings,
    ) -> Result<Vec<TrackMatch>> {
        let mut url = reqwest::Url::parse(ITUNES_SEARCH_BASE)?;
        url.query_pairs_mut()
            .append_pair("term", query)
            .append_pair("country", &settings.storefront.to_uppercase())
            .append_pair("media", "music")
            .append_pair("entity", "song")
            .append_pair("limit", "25")
            .append_pair("explicit", "Yes");

        let response = self.client.get(url).send().await?;
        let response = response
            .error_for_status()
            .context("iTunes Search API request failed")?;
        let payload: ItunesResponse = response.json().await?;

        let mut matches = payload
            .results
            .into_iter()
            .filter(|song| {
                song.wrapper_type.as_deref() == Some("track")
                    && song.kind.as_deref() == Some("song")
            })
            .filter_map(|song| {
                let score = score_track(query, &song);
                song.into_track_match(&settings.storefront)
                    .map(|track| (track, score))
            })
            .collect::<Vec<_>>();

        matches.sort_by(|left, right| right.1.cmp(&left.1));
        Ok(matches.into_iter().map(|(track, _)| track).collect())
    }

    async fn fetch_web_search_tracks(
        &self,
        query: &str,
        settings: &AppleMusicSettings,
    ) -> Result<Vec<TrackMatch>> {
        let search_url = Self::build_search_url(query, &settings.storefront);
        let response = self.client.get(&search_url).send().await?;
        let response = response
            .error_for_status()
            .context("Apple Music web search request failed")?;
        let html = response.text().await?;
        let document = Html::parse_document(&html);
        let song_selector =
            Selector::parse("[data-testid='top-search-result'][aria-label*='Song']")
                .expect("valid selector");
        let link_selector = Selector::parse("a.click-action[href*='/album/'][href*='?i=']")
            .expect("valid selector");

        let mut matches = Vec::new();
        for element in document.select(&song_selector) {
            let Some(label) = element.value().attr("aria-label") else {
                continue;
            };
            let Some(link) = element
                .select(&link_selector)
                .next()
                .and_then(|anchor| anchor.value().attr("href"))
            else {
                continue;
            };

            let Some((title, artist)) = parse_song_aria_label(label) else {
                continue;
            };
            let Some(track_id) = extract_apple_music_track_id(link) else {
                continue;
            };

            let url = normalize_apple_music_link(link)?;

            matches.push(TrackMatch {
                id: track_id,
                title,
                artist_name: artist,
                album_name: String::new(),
                duration_ms: None,
                url,
                artwork_url: None,
            });
        }

        if matches.is_empty() {
            return Ok(matches);
        }

        matches.sort_by(|left, right| {
            score_web_track(query, right)
                .cmp(&score_web_track(query, left))
                .then_with(|| left.title.cmp(&right.title))
        });
        matches.dedup_by(|left, right| left.id == right.id || left.url == right.url);
        Ok(matches)
    }

    pub fn build_search_url(query: &str, storefront: &str) -> String {
        let mut url = reqwest::Url::parse(&format!(
            "https://music.apple.com/{}/search",
            if storefront.trim().is_empty() {
                "us"
            } else {
                storefront
            }
        ))
        .expect("valid Apple Music search url");
        url.query_pairs_mut()
            .append_pair("term", query)
            .append_pair("app", "music");
        url.to_string()
    }
}

fn normalize_apple_music_link(link: &str) -> Result<String> {
    let base = reqwest::Url::parse("https://music.apple.com")?;
    let parsed = base.join(link)?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    if parsed.scheme() != "https" || host != "music.apple.com" {
        anyhow::bail!("Apple Music search returned an unexpected link host.");
    }
    Ok(parsed.to_string())
}

#[derive(Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ItunesResponse {
    results: Vec<ItunesSong>,
}

#[derive(Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ItunesSong {
    wrapper_type: Option<String>,
    kind: Option<String>,
    track_id: Option<u64>,
    collection_id: Option<u64>,
    track_name: Option<String>,
    artist_name: Option<String>,
    collection_name: Option<String>,
    track_time_millis: Option<i64>,
    artwork_url100: Option<String>,
    primary_genre_name: Option<String>,
}

impl ItunesSong {
    fn into_track_match(self, storefront: &str) -> Option<TrackMatch> {
        let track_id = self.track_id?;
        let collection_id = self.collection_id?;
        let title = self
            .track_name
            .unwrap_or_else(|| "Unknown title".to_string());
        let artist = self
            .artist_name
            .unwrap_or_else(|| "Unknown artist".to_string());
        let album = self.collection_name.unwrap_or_default();

        Some(TrackMatch {
            id: track_id.to_string(),
            title: title.clone(),
            artist_name: artist.clone(),
            album_name: album.clone(),
            duration_ms: self.track_time_millis,
            url: build_track_url(track_id, collection_id, &title, &album, storefront),
            artwork_url: self.artwork_url100,
        })
    }
}

fn build_track_url(
    track_id: u64,
    collection_id: u64,
    title: &str,
    album: &str,
    storefront: &str,
) -> String {
    let storefront = if storefront.trim().is_empty() {
        "us"
    } else {
        storefront
    };
    let slug = slugify(if album.trim().is_empty() {
        title
    } else {
        album
    });
    format!(
        "https://music.apple.com/{}/album/{}/{collection_id}?i={track_id}&app=music",
        storefront.to_lowercase(),
        slug
    )
}

fn slugify(value: &str) -> String {
    let normalized = normalize_text(value);
    if normalized.is_empty() {
        "track".to_string()
    } else {
        normalized.replace(' ', "-")
    }
}

/// Markers that identify karaoke/cover/tribute recordings. These regularly
/// carry the requested artist's name in their own title (e.g. "Human Nature
/// Michael Jackson" by a karaoke act), which otherwise wins exact-title
/// bonuses over the real song.
const IMITATION_MARKERS: &[&str] = &[
    "karaoke",
    "originally performed",
    "in the style of",
    "as made famous",
    "made famous by",
    "tribute to",
    "tribute band",
    "cover version",
    "instrumental version",
    "backing track",
    "lullaby",
    "rockabye",
    "kidz bop",
    "music box version",
    "8 bit",
];

fn imitation_penalty(query_normalized: &str, title: &str, artist: &str, album: &str) -> i32 {
    let query_wants_imitation = IMITATION_MARKERS
        .iter()
        .any(|marker| query_normalized.contains(marker))
        || query_normalized.contains("cover")
        || query_normalized.contains("instrumental");
    if query_wants_imitation {
        return 0;
    }

    let is_imitation = IMITATION_MARKERS
        .iter()
        .any(|marker| title.contains(marker) || artist.contains(marker) || album.contains(marker));
    if is_imitation {
        -600
    } else {
        0
    }
}

/// Words that mark a cover wherever they appear as a whole word (unlike
/// "cover" inside "Undercover").
const COVER_WORDS: &[&str] = &["cover", "covers", "tribute"];

fn is_cover(title: &str, artist: &str, album: &str) -> bool {
    [title, artist, album].iter().any(|field| {
        field
            .split_whitespace()
            .any(|word| COVER_WORDS.contains(&word))
    })
}

/// Whether the request names this artist ("redbone childish gambino" names
/// Childish Gambino).
fn artist_named_in(query: &str, artist: &str) -> bool {
    let query = strip_featuring(&normalize_text(query));
    let artist = normalize_text(artist);
    !artist.is_empty() && format!(" {query} ").contains(&format!(" {artist} "))
}

/// False when the request names an artist (as one of the candidates'
/// artists) but the best pick is someone else's recording.
fn is_confident(query: &str, ranked: &[TrackMatch]) -> bool {
    let Some(best) = ranked.first() else {
        return false;
    };
    let named = ranked
        .iter()
        .any(|track| artist_named_in(query, &track.artist_name));
    !named || artist_named_in(query, &best.artist_name)
}

fn score_track(query: &str, song: &ItunesSong) -> i32 {
    let query_normalized = strip_featuring(&normalize_text(query));
    let title = strip_featuring(&normalize_text(
        song.track_name.as_deref().unwrap_or_default(),
    ));
    let artist = normalize_text(song.artist_name.as_deref().unwrap_or_default());
    let album = normalize_text(song.collection_name.as_deref().unwrap_or_default());
    let mut score = imitation_penalty(&query_normalized, &title, &artist, &album);
    let wants_cover = COVER_WORDS
        .iter()
        .any(|word| query_normalized.split_whitespace().any(|token| token == *word));
    if score == 0 && !wants_cover && is_cover(&title, &artist, &album) {
        score -= 600;
    }

    if title == query_normalized {
        // A title that swallows the entire query while the artist shares no
        // words with it is usually a cover named after the request.
        score += if artist.is_empty() || token_overlap(&artist, &query_normalized) > 0.0 {
            300
        } else {
            60
        };
    }
    if format!("{artist} {title}") == query_normalized
        || format!("{title} {artist}") == query_normalized
    {
        score += 240;
    }
    if artist == query_normalized {
        score += 50;
    }
    if !title.is_empty() && query_normalized.contains(&title) {
        score += 120;
    }
    if !query_normalized.is_empty() && title.contains(&query_normalized) {
        score += 80;
    }
    if !artist.is_empty() && query_normalized.contains(&artist) {
        score += 70;
    }
    if !query_normalized.is_empty() && artist.contains(&query_normalized) {
        score += 30;
    }
    if !album.is_empty() && query_normalized.contains(&album) {
        score += 15;
    }

    for term in query_normalized.split_whitespace() {
        if title.contains(term) {
            score += 18;
        }
        if artist.contains(term) {
            score += 10;
        }
        if album.contains(term) {
            score += 4;
        }
    }

    score += (token_overlap(&title, &query_normalized) * 160.0).round() as i32;
    score += (token_overlap(&artist, &query_normalized) * 90.0).round() as i32;

    if !query_normalized.contains("live")
        && !query_normalized.contains("edit")
        && !query_normalized.contains("remix")
        && !query_normalized.contains("version")
        && !query_normalized.contains("karaoke")
        && (title.contains("live")
            || title.contains("edit")
            || title.contains("remix")
            || title.contains("version")
            || title.contains("karaoke"))
    {
        score -= 40;
    }

    if !query_normalized.contains("soundtrack")
        && !query_normalized.contains("from ")
        && (album.contains("soundtrack") || album.contains("from "))
    {
        score -= 8;
    }

    if song.primary_genre_name.as_deref() == Some("Hip-Hop/Rap") {
        score += 2;
    }

    score
}

fn score_web_track(query: &str, track: &TrackMatch) -> i32 {
    let query_normalized = strip_featuring(&normalize_text(query));
    let title = strip_featuring(&normalize_text(&track.title));
    let artist = normalize_text(&track.artist_name);
    let mut score = imitation_penalty(&query_normalized, &title, &artist, "");

    if title == query_normalized {
        score += 300;
    }
    if format!("{artist} {title}") == query_normalized
        || format!("{title} {artist}") == query_normalized
    {
        score += 240;
    }
    if !title.is_empty() && query_normalized.contains(&title) {
        score += 120;
    }
    if !artist.is_empty() && query_normalized.contains(&artist) {
        score += 70;
    }
    score += (token_overlap(&title, &query_normalized) * 160.0).round() as i32;
    score += (token_overlap(&artist, &query_normalized) * 90.0).round() as i32;
    score
}

fn parse_song_aria_label(value: &str) -> Option<(String, String)> {
    let normalized = value
        .replace('\u{00A0}', " ")
        .replace('\u{2004}', " ")
        .replace('\u{2014}', " ")
        .replace('\u{00B7}', "|");
    let parts = normalized
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.len() < 3 || !parts[1].eq_ignore_ascii_case("song") {
        return None;
    }

    Some((parts[0].to_string(), parts[2].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn itunes(title: &str, artist: &str, album: &str) -> ItunesSong {
        ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some(title.to_string()),
            artist_name: Some(artist.to_string()),
            collection_name: Some(album.to_string()),
            ..Default::default()
        }
    }

    fn matched(id: &str, title: &str, artist: &str) -> TrackMatch {
        TrackMatch {
            id: id.to_string(),
            title: title.to_string(),
            artist_name: artist.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn covers_named_in_the_album_are_marked_down() {
        let query = "redbone childish gambino";
        let cover = itunes("Redbone", "Smooth Jazz All Stars", "Smooth Jazz All Stars Cover Childish Gambino");
        let piano = itunes("Redbone (Piano Arrangement)", "The Theorist", "Piano Covers, Vol. 8");
        let real = itunes("Redbone", "Childish Gambino", "\"Awaken, My Love!\"");
        assert!(score_track(query, &cover) < 0);
        assert!(score_track(query, &piano) < 0);
        assert!(score_track(query, &real) > 300);
        // Asking for a cover still finds one.
        assert!(score_track("redbone smooth jazz cover", &cover) > 0);
        // "Cover" inside a word isn't a cover.
        assert!(!is_cover("undercover", "kid cudi", "man on the moon"));
    }

    #[test]
    fn a_named_artist_that_isnt_the_pick_triggers_a_second_look() {
        let query = "Redbone Childish Gambino";
        let ranked = vec![
            matched("1", "Redbone", "Lofi Fruits Music"),
            matched("2", "Me and Your Mama", "Childish Gambino"),
        ];
        assert!(!is_confident(query, &ranked));

        let good = vec![matched("3", "Redbone", "Childish Gambino")];
        assert!(is_confident(query, &good));

        // No artist named: nothing to check against.
        assert!(is_confident("redbone", &[matched("1", "Redbone", "Lofi Fruits Music")]));
        // Artists match on whole words only.
        assert!(!artist_named_in("human nature michael jackson", "Jackson 5"));
    }

    #[test]
    fn score_prefers_exact_title_artist() {
        let song = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Human Nature".to_string()),
            artist_name: Some("Michael Jackson".to_string()),
            collection_name: Some("Thriller".to_string()),
            ..Default::default()
        };

        assert!(score_track("human nature michael jackson", &song) > 200);
    }

    #[test]
    fn real_track_outranks_karaoke_named_after_query() {
        let real = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Human Nature".to_string()),
            artist_name: Some("Michael Jackson".to_string()),
            collection_name: Some("Thriller".to_string()),
            ..Default::default()
        };
        let karaoke_exact_title = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Human Nature Michael Jackson".to_string()),
            artist_name: Some("Party Tyme Karaoke".to_string()),
            collection_name: Some("Karaoke Super Hits".to_string()),
            ..Default::default()
        };
        let karaoke_styled = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some(
                "Human Nature (Originally Performed by Michael Jackson) [Karaoke Version]"
                    .to_string(),
            ),
            artist_name: Some("Karaoke Cloud".to_string()),
            collection_name: Some("Karaoke Hits".to_string()),
            ..Default::default()
        };

        let query = "human nature michael jackson";
        assert!(score_track(query, &real) > score_track(query, &karaoke_exact_title));
        assert!(score_track(query, &real) > score_track(query, &karaoke_styled));
    }

    #[test]
    fn featured_artist_title_outranks_lullaby_cover() {
        // Real-world failure: "Uptown Funk Bruno Mars" matched the Rockabye
        // Baby! lullaby because the real track credits Mark Ronson with Bruno
        // Mars only in the title's feat clause.
        let real = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Uptown Funk (feat. Bruno Mars)".to_string()),
            artist_name: Some("Mark Ronson".to_string()),
            collection_name: Some("Uptown Special".to_string()),
            ..Default::default()
        };
        let lullaby = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Uptown Funk".to_string()),
            artist_name: Some("Rockabye Baby!".to_string()),
            collection_name: Some("Lullaby Renditions of Bruno Mars".to_string()),
            ..Default::default()
        };

        let query = "uptown funk bruno mars";
        assert!(score_track(query, &real) > score_track(query, &lullaby));
    }

    #[test]
    fn karaoke_request_still_finds_karaoke() {
        let karaoke = ItunesSong {
            wrapper_type: Some("track".to_string()),
            kind: Some("song".to_string()),
            track_name: Some("Human Nature (Karaoke Version)".to_string()),
            artist_name: Some("Karaoke Cloud".to_string()),
            collection_name: Some("Karaoke Hits".to_string()),
            ..Default::default()
        };

        assert!(score_track("human nature karaoke", &karaoke) > 0);
    }

    #[test]
    fn builds_search_url() {
        let url = AppleCatalog::build_search_url("human nature", "us");
        assert!(url.contains("music.apple.com/us/search"));
    }

    #[test]
    fn normalizes_relative_apple_music_links() {
        let url = normalize_apple_music_link("/us/album/example/1?i=2").unwrap();
        assert_eq!(url, "https://music.apple.com/us/album/example/1?i=2");
    }

    #[test]
    fn parses_song_aria_label() {
        let parsed = parse_song_aria_label(
            "Overqualified\u{2004}\u{00B7}\u{2004}Song\u{2004}\u{00B7}\u{2004}Durand Bernarr",
        );
        assert_eq!(
            parsed,
            Some(("Overqualified".to_string(), "Durand Bernarr".to_string()))
        );
    }
}
