//! Listen together, the guest's side: two streamers live together play the
//! same music at the same moment. The guest follows the host's "Now playing"
//! posts and plays each song in their own player, in step with the host.
//!
//! The host's post goes out as the song starts and takes a moment to arrive,
//! so the song started for the host at about `post arrived - ANNOUNCE_LAG`.
//! The guest can nudge that by a small offset if one side sounds ahead.

use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::{
    models::ListenStatus,
    services::{listen::parse_now_playing, twitch_service::FollowEvent},
};

pub const DEFAULT_OFFSET_MS: i64 = 0;
/// The fine-tune range, either way.
const MAX_OFFSET_MS: i64 = 3_000;
/// How long after a song starts its post typically reaches the guest.
const ANNOUNCE_LAG_MS: i64 = 400;

impl AppContext {
    pub async fn listen_start(self: &Arc<Self>, channel: &str, offset_ms: i64) -> AppState {
        let channel = channel.trim().trim_start_matches('#').to_lowercase();
        self.listen_stop_task().await;
        if channel.is_empty() {
            self.set_listen(|listen| {
                listen.phase = "error".to_string();
                listen.detail = "Enter the channel to listen along with.".to_string();
            })
            .await;
            return self.snapshot().await;
        }

        self.set_listen(|listen| {
            *listen = ListenStatus {
                phase: "connecting".to_string(),
                channel: channel.clone(),
                detail: format!("Connecting to #{channel}..."),
                offset_ms: offset_ms.clamp(-MAX_OFFSET_MS, MAX_OFFSET_MS),
                ..Default::default()
            };
        })
        .await;
        self.add_log(LogLevel::Info, format!("Listen together: following #{channel}."))
            .await;

        let (event_tx, event_rx) = mpsc::channel::<FollowEvent>(64);
        let follower = tauri::async_runtime::spawn(twitch_service::follow_channel(
            channel.clone(),
            event_tx,
        ));
        let context = Arc::clone(self);
        let task = tauri::async_runtime::spawn(async move {
            context.follow(channel, event_rx).await;
            follower.abort();
        });
        *self.listen_task.lock().await = Some(task);
        self.snapshot().await
    }

    pub async fn listen_stop(self: &Arc<Self>) -> AppState {
        let following = self.runtime.read().await.listen_song.is_some();
        self.listen_stop_task().await;
        if following {
            let _ = self.player_bridge.run_command(&self.handle, "pause", None).await;
        }
        let was_on = {
            let runtime = self.runtime.read().await;
            runtime.listen.phase != "off"
        };
        self.set_listen(|listen| {
            listen.phase = "off".to_string();
            listen.detail.clear();
            listen.title.clear();
            listen.artist.clear();
            listen.requested_by = None;
        })
        .await;
        if was_on {
            self.add_log(LogLevel::Info, "Listen together: stopped.").await;
            self.ensure_queue_progress("listen together stopped").await;
        }
        self.snapshot().await
    }

    /// A new offset applies to the song already playing too.
    pub async fn listen_set_offset(self: &Arc<Self>, offset_ms: i64) -> AppState {
        let offset_ms = offset_ms.clamp(-MAX_OFFSET_MS, MAX_OFFSET_MS);
        self.set_listen(|listen| listen.offset_ms = offset_ms).await;
        let song = {
            let mut runtime = self.runtime.write().await;
            runtime.listen_generation += 1;
            runtime.listen_song.clone().map(|song| (song, runtime.listen_generation))
        };
        if let Some(((id, received), generation)) = song {
            tauri::async_runtime::spawn(Arc::clone(self).play_followed(id, received, generation));
        }
        self.snapshot().await
    }

    async fn listen_stop_task(&self) {
        if let Some(task) = self.listen_task.lock().await.take() {
            task.abort();
        }
        let mut runtime = self.runtime.write().await;
        runtime.listen_song = None;
        runtime.listen_generation += 1;
    }

    async fn follow(self: Arc<Self>, channel: String, mut events: mpsc::Receiver<FollowEvent>) {
        while let Some(event) = events.recv().await {
            match event {
                FollowEvent::Connected => {
                    let waiting = self.runtime.read().await.listen_song.is_none();
                    self.set_listen(|listen| {
                        listen.phase = if waiting { "waiting" } else { "playing" }.to_string();
                        listen.detail = if waiting {
                            format!("Connected to #{channel}. Waiting for the next song...")
                        } else {
                            format!("Following #{channel}.")
                        };
                    })
                    .await;
                }
                FollowEvent::Retrying(reason) => {
                    self.set_listen(|listen| {
                        listen.phase = "connecting".to_string();
                        listen.detail = reason;
                    })
                    .await;
                }
                FollowEvent::Chat(message) => {
                    let Some(post) = parse_now_playing(&message.message) else {
                        continue;
                    };
                    // Anyone can type a post; only the streamer's own
                    // AppleCrap (the channel or a mod bot) is followed.
                    if !message.is_mod_or_broadcaster {
                        self.add_log(
                            LogLevel::Warn,
                            format!(
                                "Listen together: ignored a \"Now playing\" post from @{} (not the streamer or a mod).",
                                message.login
                            ),
                        )
                        .await;
                        continue;
                    }
                    self.follow_post(post, SystemTime::now()).await;
                }
            }
        }
    }

    async fn follow_post(self: &Arc<Self>, post: crate::services::listen::NowPlayingPost, received: SystemTime) {
        let label = if post.artist.is_empty() {
            post.title.clone()
        } else {
            format!("{} \u{2014} {}", post.title, post.artist)
        };
        let Some(id) = post.catalog_id.clone() else {
            self.set_listen(|listen| {
                listen.detail = format!("Couldn't follow \"{label}\": the post has no Apple Music link.");
            })
            .await;
            return;
        };
        let generation = {
            let mut runtime = self.runtime.write().await;
            runtime.listen_generation += 1;
            runtime.listen_song = Some((id.clone(), received));
            runtime.listen_generation
        };
        self.set_listen(|listen| {
            listen.phase = "playing".to_string();
            listen.detail = format!("Starting \"{label}\"...");
            listen.title = post.title.clone();
            listen.artist = post.artist.clone();
            listen.requested_by = post.requested_by.clone();
        })
        .await;
        self.add_log(LogLevel::Info, format!("Listen together: {label}."))
            .await;
        tauri::async_runtime::spawn(Arc::clone(self).play_followed(id, received, generation));
    }

    /// Waits until the song should start for the listener, then plays it
    /// (or seeks it, if it's already playing) from the right spot.
    async fn play_followed(self: Arc<Self>, id: String, received: SystemTime, generation: u64) {
        let offset_ms = self.runtime.read().await.listen.offset_ms;
        let start_at_ms = epoch_ms(received) - ANNOUNCE_LAG_MS + offset_ms;
        let wait = start_at_ms - epoch_ms(SystemTime::now());
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait as u64)).await;
        }
        if self.runtime.read().await.listen_generation != generation {
            return;
        }

        let payload = serde_json::json!({ "song": id, "startAtMs": start_at_ms }).to_string();
        let outcome = self
            .player_bridge
            .run_command(&self.handle, "listenPlay", Some(&payload))
            .await;
        if self.runtime.read().await.listen_generation != generation {
            return;
        }
        let detail = match outcome {
            Ok(position) => {
                self.add_log(LogLevel::Info, format!("Listen together: playing {position}."))
                    .await;
                let runtime = self.runtime.read().await;
                format!("Following #{}.", runtime.listen.channel)
            }
            Err(error) => format!("The player couldn't play it: {error}"),
        };
        self.set_listen(|listen| listen.detail = detail).await;
    }

    async fn set_listen(&self, change: impl FnOnce(&mut ListenStatus)) {
        {
            let mut runtime = self.runtime.write().await;
            change(&mut runtime.listen);
        }
        self.emit_state().await;
    }
}

fn epoch_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}
