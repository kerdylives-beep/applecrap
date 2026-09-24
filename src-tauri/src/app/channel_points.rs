//! Channel Points song requests: keeping the app's reward in line with the
//! settings, listening for redemptions, and settling each one — fulfilled
//! when the song is queued, refunded when it can't be.

use std::future::Future;

use crate::{
    models::{
        AuthSlot, ChannelPointsPhase, ChannelPointsSettings, ChannelPointsState, TwitchAuth,
    },
    services::{
        channel_points::{self, HelixAuth, HelixError, Redemption, RewardSpec},
        eventsub::{self, SessionEnd},
        twitch_auth,
    },
};

use super::*;

const MANAGE_SCOPE: &str = "channel:manage:redemptions";
/// Redemptions this old when the app catches up are refunded rather than
/// queued: the stream they were meant for is most likely over.
const STALE_REDEMPTION_HOURS: i64 = 6;
/// Redemption ids remembered so a redemption seen both live and in a
/// catch-up is only handled once.
const SEEN_REDEMPTIONS: usize = 256;
/// Wait before retrying setup that failed for want of a connection.
const SETUP_RETRY: Duration = Duration::from_secs(60);

fn reward_prompt(allow_links: bool) -> &'static str {
    if allow_links {
        "Type a song name and artist, or paste an Apple Music link."
    } else {
        "Type a song name and artist."
    }
}

fn is_stale(redeemed_at: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(redeemed_at)
        .map(|at| {
            chrono::Utc::now().signed_duration_since(at)
                > chrono::Duration::hours(STALE_REDEMPTION_HOURS)
        })
        .unwrap_or(false)
}

/// 2s, 4s, 8s, 16s, 32s, then 60s between attempts.
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2_u64.saturating_pow(attempt.clamp(1, 6)).min(60))
}

impl AppContext {
    /// Brings Channel Points in line with the settings and sign-in: creates
    /// or updates the reward and listens for redemptions, or hides the reward
    /// and stops listening. Safe to call any time; calls are serialized.
    pub async fn sync_channel_points(self: &Arc<Self>) {
        let _turn = self.channel_points_sync.lock().await;
        if let Some(retry) = self.channel_points_retry.lock().await.take() {
            retry.abort();
        }

        let (settings, allow_links, broadcaster, stored) = {
            let persisted = self.persisted.read().await;
            (
                persisted.settings.channel_points.clone(),
                persisted.settings.request_limits.allow_links,
                persisted.auth.broadcaster.clone(),
                persisted.channel_points.clone(),
            )
        };
        let manager = broadcaster
            .filter(|account| account.scopes.iter().any(|scope| scope == MANAGE_SCOPE));

        if !settings.enabled {
            self.stop_channel_points_listener().await;
            if let Some(account) = manager.as_ref() {
                self.disable_owned_reward(account, &stored).await;
            }
            self.set_channel_points_status(ChannelPointsPhase::Off, "").await;
            return;
        }
        let Some(account) = manager else {
            self.stop_channel_points_listener().await;
            self.set_channel_points_status(
                ChannelPointsPhase::Error,
                "Sign in as the broadcaster (Bot Setup) to take Channel Points requests.",
            )
            .await;
            return;
        };

        if !self.channel_points_listening(&stored).await {
            self.set_channel_points_status(ChannelPointsPhase::Starting, "Setting up the reward...")
                .await;
        }
        let reward_id = match self
            .ensure_reward(&account, &stored, &settings, reward_prompt(allow_links))
            .await
        {
            Ok(reward_id) => reward_id,
            Err(error) => {
                self.stop_channel_points_listener().await;
                let message = error.user_message(&settings.title);
                self.set_channel_points_status(ChannelPointsPhase::Error, &message)
                    .await;
                self.add_log(LogLevel::Warn, format!("Channel Points: {message}"))
                    .await;
                if error.is_transient() {
                    self.schedule_channel_points_retry().await;
                }
                return;
            }
        };
        self.start_channel_points_listener(account.user_id.clone(), reward_id)
            .await;
    }

    async fn schedule_channel_points_retry(self: &Arc<Self>) {
        let context = Arc::clone(self);
        let retry = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SETUP_RETRY).await;
            // Leave the slot empty so the sync below doesn't abort this task.
            context.channel_points_retry.lock().await.take();
            context.boxed_sync().await;
        });
        *self.channel_points_retry.lock().await = Some(retry);
    }

    /// `sync_channel_points` behind a boxed future: it schedules its own
    /// retry, and the compiler can't prove a future that contains itself is
    /// `Send` without this indirection.
    fn boxed_sync(self: Arc<Self>) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { self.sync_channel_points().await })
    }

    /// Hides the reward and stops listening. Runs before the broadcaster
    /// signs out, while there's still a token to do it with — otherwise the
    /// reward would stay up with nobody to settle redemptions.
    pub(super) async fn retire_channel_points(&self) {
        let _turn = self.channel_points_sync.lock().await;
        self.stop_channel_points_listener().await;
        let (broadcaster, stored) = {
            let persisted = self.persisted.read().await;
            (persisted.auth.broadcaster.clone(), persisted.channel_points.clone())
        };
        if let Some(account) = broadcaster {
            self.disable_owned_reward(&account, &stored).await;
        }
    }

    /// Whether `points_only` should turn `!request` away: only while
    /// redemptions are actually being taken.
    pub async fn channel_points_live(&self) -> bool {
        self.runtime.read().await.channel_points_status.phase == ChannelPointsPhase::Live
    }

    async fn set_channel_points_status(&self, phase: ChannelPointsPhase, detail: &str) {
        {
            let mut runtime = self.runtime.write().await;
            let status = &mut runtime.channel_points_status;
            if status.phase == phase && status.detail == detail {
                return;
            }
            status.phase = phase;
            status.detail = detail.to_string();
        }
        self.emit_state().await;
    }

    async fn set_live_status(&self) {
        let settings = self.current_settings().await.channel_points;
        self.set_channel_points_status(
            ChannelPointsPhase::Live,
            &format!(
                "Taking requests through \"{}\" ({} points).",
                settings.title, settings.cost
            ),
        )
        .await;
    }

    /// Runs a Helix call with the broadcaster's token, refreshing it once if
    /// Twitch rejects it.
    async fn with_broadcaster<T, F, Fut>(&self, call: F) -> Result<T, HelixError>
    where
        F: Fn(&'static str, String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let client_id = twitch_auth::client_id()
            .ok_or_else(|| HelixError::SignIn("Sign in with Twitch isn't set up in this build.".into()))?;
        let token = self
            .access_token(AuthSlot::Broadcaster)
            .await
            .map_err(sign_in_error)?;
        match call(client_id, token.clone()).await {
            Err(HelixError::Unauthorized) => {
                let fresh = self
                    .refresh_after_rejection(AuthSlot::Broadcaster, &token)
                    .await
                    .map_err(sign_in_error)?;
                call(client_id, fresh).await
            }
            other => other,
        }
    }

    async fn ensure_reward(
        &self,
        account: &TwitchAuth,
        stored: &ChannelPointsState,
        settings: &ChannelPointsSettings,
        prompt: &str,
    ) -> Result<String, HelixError> {
        let http = &self.http;
        let broadcaster_id = account.user_id.as_str();
        let spec = RewardSpec {
            title: &settings.title,
            cost: settings.cost,
            prompt,
        };
        let spec = &spec;

        let owned = stored
            .reward_id
            .as_deref()
            .filter(|_| stored.broadcaster_id.as_deref() == Some(broadcaster_id));
        if let Some(reward_id) = owned {
            let updated = self
                .with_broadcaster(|client_id, token| async move {
                    let auth = HelixAuth { client_id, token: &token };
                    channel_points::update_reward(http, &auth, broadcaster_id, reward_id, spec).await
                })
                .await;
            match updated {
                Ok(()) => return Ok(reward_id.to_string()),
                // Deleted on Twitch's side: make a fresh one below.
                Err(HelixError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }

        let reward_id = self
            .with_broadcaster(|client_id, token| async move {
                let auth = HelixAuth { client_id, token: &token };
                channel_points::create_reward(http, &auth, broadcaster_id, spec).await
            })
            .await?;
        self.persisted.write().await.channel_points = ChannelPointsState {
            reward_id: Some(reward_id.clone()),
            broadcaster_id: Some(broadcaster_id.to_string()),
        };
        let _ = self.save_persisted().await;
        self.add_log(
            LogLevel::Info,
            format!(
                "Created the Channel Points reward \"{}\" ({} points).",
                settings.title, settings.cost
            ),
        )
        .await;
        Ok(reward_id)
    }

    /// Hides the reward, if this app made it for `account`.
    async fn disable_owned_reward(&self, account: &TwitchAuth, stored: &ChannelPointsState) {
        let Some(reward_id) = stored.reward_id.as_deref() else {
            return;
        };
        if stored.broadcaster_id.as_deref() != Some(account.user_id.as_str()) {
            return;
        }
        let http = &self.http;
        let broadcaster_id = account.user_id.as_str();
        let result = self
            .with_broadcaster(|client_id, token| async move {
                let auth = HelixAuth { client_id, token: &token };
                channel_points::disable_reward(http, &auth, broadcaster_id, reward_id).await
            })
            .await;
        match result {
            Ok(()) | Err(HelixError::NotFound) => {}
            Err(error) => {
                self.add_log(
                    LogLevel::Warn,
                    format!("Couldn't hide the Channel Points reward: {error}. You can turn it off in your Twitch dashboard."),
                )
                .await;
            }
        }
    }

    // --- Listener -------------------------------------------------------------

    async fn channel_points_listening(&self, stored: &ChannelPointsState) -> bool {
        let listener = self.channel_points_listener.lock().await;
        matches!(
            (listener.as_ref(), stored.reward_id.as_deref()),
            (Some((listening, task)), Some(reward)) if listening == reward && !task.inner().is_finished()
        )
    }

    async fn start_channel_points_listener(self: &Arc<Self>, broadcaster_id: String, reward_id: String) {
        let mut listener = self.channel_points_listener.lock().await;
        if let Some((listening, task)) = listener.as_ref() {
            if *listening == reward_id && !task.inner().is_finished() {
                drop(listener);
                // Title or cost may have changed; the listener is untouched.
                self.set_live_status().await;
                return;
            }
        }
        if let Some((_, task)) = listener.take() {
            task.abort();
        }
        let context = Arc::clone(self);
        let listening = reward_id.clone();
        let task = tauri::async_runtime::spawn(async move {
            context.listen_for_redemptions(broadcaster_id, reward_id).await;
        });
        *listener = Some((listening, task));
    }

    async fn stop_channel_points_listener(&self) {
        if let Some((_, task)) = self.channel_points_listener.lock().await.take() {
            task.abort();
        }
    }

    async fn listen_for_redemptions(self: Arc<Self>, broadcaster_id: String, reward_id: String) {
        // Redemptions are settled one at a time, in order, off the socket's
        // task so a slow catalog lookup never delays reading keepalives. The
        // worker drains what it has even if the listener is stopped: those
        // viewers already paid.
        let (redemptions_tx, mut redemptions_rx) = mpsc::unbounded_channel::<Redemption>();
        let worker = Arc::clone(&self);
        let (worker_broadcaster, worker_reward) = (broadcaster_id.clone(), reward_id.clone());
        tauri::async_runtime::spawn(async move {
            while let Some(redemption) = redemptions_rx.recv().await {
                worker
                    .settle_redemption(&worker_broadcaster, &worker_reward, redemption)
                    .await;
            }
        });

        let mut attempt: u32 = 0;
        let mut carried: Option<(eventsub::Socket, Duration)> = None;
        loop {
            let (mut socket, keepalive) = match carried.take() {
                Some(session) => session,
                None => match self.open_redemption_session(&broadcaster_id, &reward_id).await {
                    Ok(session) => {
                        attempt = 0;
                        self.set_live_status().await;
                        self.catch_up_redemptions(&broadcaster_id, &reward_id, &redemptions_tx)
                            .await;
                        session
                    }
                    Err(error) if error.is_transient() => {
                        attempt = attempt.saturating_add(1);
                        let delay = backoff(attempt);
                        self.set_channel_points_status(
                            ChannelPointsPhase::Starting,
                            &format!("Reconnecting to Twitch in {}s ({error}).", delay.as_secs()),
                        )
                        .await;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    Err(error) => {
                        let title = self.current_settings().await.channel_points.title;
                        let message = error.user_message(&title);
                        self.set_channel_points_status(ChannelPointsPhase::Error, &message)
                            .await;
                        self.add_log(
                            LogLevel::Warn,
                            format!("Channel Points requests stopped: {message}"),
                        )
                        .await;
                        return;
                    }
                },
            };

            let sender = redemptions_tx.clone();
            let end = eventsub::read_until_end(&mut socket, keepalive, |redemption| {
                let _ = sender.send(redemption);
            })
            .await;

            let reason = match end {
                SessionEnd::Reconnect(url) if eventsub::is_twitch_eventsub_url(&url) => {
                    // Twitch is moving us: subscriptions carry over as long
                    // as the old socket stays open until the new one is up.
                    match eventsub::open(&url).await {
                        Ok((next, _, next_keepalive)) => {
                            drop(socket);
                            carried = Some((next, next_keepalive));
                            continue;
                        }
                        Err(error) => format!("couldn't follow Twitch's reconnect ({error})"),
                    }
                }
                SessionEnd::Reconnect(url) => format!("ignored an unexpected reconnect address ({url})"),
                SessionEnd::Revoked(reason) => {
                    let message = format!(
                        "Twitch stopped sending Channel Points redemptions ({reason}). Sign in again as the broadcaster."
                    );
                    self.set_channel_points_status(ChannelPointsPhase::Error, &message)
                        .await;
                    self.add_log(LogLevel::Warn, message).await;
                    return;
                }
                SessionEnd::Closed => "Twitch closed the connection".to_string(),
                SessionEnd::Stale => "Twitch stopped responding".to_string(),
                SessionEnd::Io(error) => format!("connection problem: {error}"),
            };
            drop(socket);
            attempt = attempt.saturating_add(1);
            let delay = backoff(attempt);
            self.add_log(
                LogLevel::Warn,
                format!("Channel Points listener: {reason}; reconnecting in {}s.", delay.as_secs()),
            )
            .await;
            self.set_channel_points_status(
                ChannelPointsPhase::Starting,
                &format!("Reconnecting to Twitch in {}s.", delay.as_secs()),
            )
            .await;
            tokio::time::sleep(delay).await;
        }
    }

    /// Opens an EventSub session and subscribes it to the reward.
    async fn open_redemption_session(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
    ) -> Result<(eventsub::Socket, Duration), HelixError> {
        let (socket, session_id, keepalive) = eventsub::open(eventsub::EVENTSUB_URL)
            .await
            .map_err(|error| HelixError::Connection(error.to_string()))?;
        let http = &self.http;
        let session_id = session_id.as_str();
        self.with_broadcaster(|client_id, token| async move {
            let auth = HelixAuth { client_id, token: &token };
            channel_points::subscribe_redemptions(http, &auth, broadcaster_id, reward_id, session_id)
                .await
        })
        .await?;
        Ok((socket, keepalive))
    }

    /// Queues redemptions that arrived while nobody was listening.
    async fn catch_up_redemptions(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        redemptions: &mpsc::UnboundedSender<Redemption>,
    ) {
        let http = &self.http;
        let waiting = self
            .with_broadcaster(|client_id, token| async move {
                let auth = HelixAuth { client_id, token: &token };
                channel_points::unfulfilled_redemptions(http, &auth, broadcaster_id, reward_id).await
            })
            .await;
        match waiting {
            Ok(waiting) if waiting.is_empty() => {}
            Ok(waiting) => {
                self.add_log(
                    LogLevel::Info,
                    format!(
                        "Catching up on {} Channel Points request(s) made while the app wasn't listening.",
                        waiting.len()
                    ),
                )
                .await;
                for redemption in waiting {
                    let _ = redemptions.send(redemption);
                }
            }
            Err(error) => {
                self.add_log(
                    LogLevel::Warn,
                    format!("Couldn't check for missed Channel Points requests: {error}"),
                )
                .await;
            }
        }
    }

    // --- Settling -------------------------------------------------------------

    /// Remembers a redemption id; false if it was already handled.
    fn first_sighting(&self, redemption_id: &str) -> bool {
        let mut seen = self
            .seen_redemptions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if seen.iter().any(|id| id == redemption_id) {
            return false;
        }
        if seen.len() >= SEEN_REDEMPTIONS {
            seen.pop_front();
        }
        seen.push_back(redemption_id.to_string());
        true
    }

    /// Runs a redemption through the same rules as a chat request. Queued:
    /// the redemption is fulfilled. Turned away: it's cancelled, which
    /// refunds the viewer's points.
    async fn settle_redemption(
        self: &Arc<Self>,
        broadcaster_id: &str,
        reward_id: &str,
        redemption: Redemption,
    ) {
        if !self.first_sighting(&redemption.id) {
            return;
        }
        let query = redemption.user_input.trim();

        let (queued, message) = if is_stale(&redemption.redeemed_at) {
            (
                false,
                "that request waited too long for the app, so your points were refunded.".to_string(),
            )
        } else {
            let result = self
                .process_request(&redemption.user_name, query, false, "channel-points")
                .await;
            if result.ok {
                (true, result.message)
            } else {
                (false, format!("{} Your points were refunded.", result.message))
            }
        };

        self.add_log(
            if queued { LogLevel::Info } else { LogLevel::Warn },
            format!(
                "Channel Points request from @{}: {} ({message})",
                redemption.user_name,
                if query.is_empty() { "<empty request>" } else { query },
            ),
        )
        .await;

        let http = &self.http;
        let redemption_id = redemption.id.as_str();
        let settled = self
            .with_broadcaster(|client_id, token| async move {
                let auth = HelixAuth { client_id, token: &token };
                channel_points::settle_redemption(
                    http,
                    &auth,
                    broadcaster_id,
                    reward_id,
                    redemption_id,
                    queued,
                )
                .await
            })
            .await;
        if let Err(error) = settled {
            self.add_log(
                LogLevel::Warn,
                format!(
                    "Couldn't {} @{}'s redemption on Twitch: {error}. Settle it from the reward queue in your Twitch dashboard.",
                    if queued { "complete" } else { "refund" },
                    redemption.user_name
                ),
            )
            .await;
        }

        self.send_chat(format!("@{} {message}", redemption.user_login))
            .await;
    }
}

/// A broadcaster-token failure, kept retryable when it was only the network.
fn sign_in_error(error: anyhow::Error) -> HelixError {
    if error.downcast_ref::<reqwest::Error>().is_some() {
        HelixError::Connection(error.to_string())
    } else {
        HelixError::SignIn(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_redemptions_are_stale() {
        let old = (chrono::Utc::now() - chrono::Duration::hours(STALE_REDEMPTION_HOURS + 1)).to_rfc3339();
        let fresh = (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        assert!(is_stale(&old));
        assert!(!is_stale(&fresh));
        assert!(!is_stale("not a date"));
    }

    #[test]
    fn backoff_caps_at_a_minute() {
        assert_eq!(backoff(1), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(40), Duration::from_secs(60));
    }

    #[test]
    fn prompt_mentions_links_only_when_allowed() {
        assert!(reward_prompt(true).contains("link"));
        assert!(!reward_prompt(false).contains("link"));
    }
}
