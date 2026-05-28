use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::Deserialize;

use crate::{
    models::{AppleMusicSettings, TrackMatch},
    services::queue_engine::{extract_apple_music_track_id, is_url, normalize_text, token_overlap},
};

const ITUNES_SEARCH_BASE: &str = "https://itunes.apple.com/search";
const ITUNES_LOOKUP_BASE: &str = "https://itunes.apple.com/lookup";

#[derive(Clone)]
pub struct AppleCatalog {
    client: Client,
}

impl AppleCatalog {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
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
            matches = self.fetch_web_search_tracks(query, settings).await?;
        }

        Ok(matches)
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

fn score_track(query: &str, song: &ItunesSong) -> i32 {
    let query_normalized = normalize_text(query);
    let title = normalize_text(song.track_name.as_deref().unwrap_or_default());
    let artist = normalize_text(song.artist_name.as_deref().unwrap_or_default());
    let album = normalize_text(song.collection_name.as_deref().unwrap_or_default());
    let mut score = 0;

    if title == query_normalized {
        score += 300;
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
    let query_normalized = normalize_text(query);
    let title = normalize_text(&track.title);
    let artist = normalize_text(&track.artist_name);
    let mut score = 0;

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
