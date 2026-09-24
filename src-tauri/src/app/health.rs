//! Notices when Apple Music stops behaving the way the app expects — most
//! likely because Apple changed its site — and says so, instead of letting
//! every request quietly fall through to "no match".

use crate::models::{Alert, AlertAction, AlertTone, TrackMatch};

use super::*;

/// A song search always finds; if it doesn't, search itself is broken.
const CANARY_QUERY: &str = "Human Nature Michael Jackson";
const CANARY_TITLE: &str = "human nature";
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const RECHECK_AFTER_FAILURE: Duration = Duration::from_secs(60);
/// A player that's still "loading" this long isn't going to finish.
const PLAYER_STUCK_AFTER: Duration = Duration::from_secs(3 * 60);

const SEARCH_ALERT: &str = "apple-search";
const PLAYER_ALERT: &str = "apple-player";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SearchHealth {
    Working,
    /// Couldn't reach Apple at all: the connection, not Apple.
    Offline,
    /// Apple answered, but not with what the app understands.
    Broken(String),
}

pub(super) fn classify_search(result: &Result<Vec<TrackMatch>>) -> SearchHealth {
    match result {
        Ok(tracks)
            if tracks
                .iter()
                .any(|track| track.title.to_lowercase().contains(CANARY_TITLE)) =>
        {
            SearchHealth::Working
        }
        Ok(_) => SearchHealth::Broken("search came back empty for a song it always finds".into()),
        Err(error) => {
            let unreachable = error.chain().any(|cause| {
                cause
                    .downcast_ref::<reqwest::Error>()
                    .is_some_and(|error| error.is_connect() || error.is_timeout())
            });
            if unreachable {
                SearchHealth::Offline
            } else {
                SearchHealth::Broken(error.to_string())
            }
        }
    }
}

impl AppContext {
    /// Asks for a search-health check now (after a real lookup failed).
    pub(super) fn nudge_apple_health(&self) {
        self.apple_health_nudge.notify_one();
    }

    /// Runs for the life of the app.
    pub(super) async fn watch_apple_search(self: Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut failures = 0u32;
        loop {
            let settings = self.current_settings().await.apple_music;
            let result = self.apple_catalog.search_tracks(CANARY_QUERY, &settings).await;
            let health = classify_search(&result);

            // One bad answer can be a blip; act on two in a row.
            failures = if health == SearchHealth::Working { 0 } else { failures + 1 };
            match health {
                SearchHealth::Working => self.clear_alert(SEARCH_ALERT).await,
                _ if failures < 2 => {}
                SearchHealth::Offline => {
                    self.raise_alert(Alert {
                        id: SEARCH_ALERT.into(),
                        tone: AlertTone::Warn,
                        title: "Can't reach Apple Music".into(),
                        detail: "Check the internet connection. Requests are matched again once it's back.".into(),
                        action: None,
                    })
                    .await;
                }
                SearchHealth::Broken(reason) => {
                    if failures == 2 {
                        self.add_log(
                            LogLevel::Error,
                            format!("Apple Music search health check failed: {reason}"),
                        )
                        .await;
                    }
                    self.raise_alert(Alert {
                        id: SEARCH_ALERT.into(),
                        tone: AlertTone::Error,
                        title: "Apple Music search isn't working".into(),
                        detail: "Requests will wait for manual review until it's fixed. Apple may have changed their site, which needs an AppleCrap update.".into(),
                        action: Some(AlertAction::ReportProblem),
                    })
                    .await;
                }
            }

            let wait = if failures > 0 { RECHECK_AFTER_FAILURE } else { CHECK_EVERY };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.apple_health_nudge.notified() => {}
            }
        }
    }

    /// Called with each player status. MusicKit missing for minutes means
    /// the page loaded but isn't the player the app knows.
    pub(super) async fn note_player_status(&self, status: &str) {
        let stuck = {
            let mut runtime = self.runtime.write().await;
            if status == "Loading" {
                let since = *runtime
                    .player_loading_since
                    .get_or_insert_with(std::time::Instant::now);
                since.elapsed() >= PLAYER_STUCK_AFTER
            } else {
                runtime.player_loading_since = None;
                false
            }
        };
        if stuck {
            self.raise_alert(Alert {
                id: PLAYER_ALERT.into(),
                tone: AlertTone::Error,
                title: "The Apple Music player isn't starting".into(),
                detail: "It's been loading for a few minutes. Restarting AppleCrap usually fixes it; if it keeps happening, Apple may have changed their web player.".into(),
                action: Some(AlertAction::ReportProblem),
            })
            .await;
        } else if status != "Loading" {
            self.clear_alert(PLAYER_ALERT).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str) -> TrackMatch {
        TrackMatch {
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn finding_the_canary_means_search_works() {
        assert_eq!(
            classify_search(&Ok(vec![track("Human Nature")])),
            SearchHealth::Working
        );
    }

    #[test]
    fn empty_or_wrong_results_mean_search_is_broken() {
        assert!(matches!(classify_search(&Ok(vec![])), SearchHealth::Broken(_)));
        assert!(matches!(
            classify_search(&Ok(vec![track("Something Else")])),
            SearchHealth::Broken(_)
        ));
    }

    #[test]
    fn an_unexpected_answer_is_broken_not_offline() {
        let parse_error: anyhow::Error = serde_json::from_str::<serde_json::Value>("<html>")
            .unwrap_err()
            .into();
        assert!(matches!(
            classify_search(&Err(parse_error)),
            SearchHealth::Broken(_)
        ));
    }

    #[tokio::test]
    async fn an_unreachable_host_is_offline() {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        // Port 9 on localhost: nothing listens, so the connection is refused.
        let error: anyhow::Error = client
            .get("http://127.0.0.1:9/")
            .send()
            .await
            .unwrap_err()
            .into();
        assert_eq!(classify_search(&Err(error)), SearchHealth::Offline);
    }
}
