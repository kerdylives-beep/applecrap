//! Request intake and the queue: validation, matching, approval, dispatch.

use super::*;

impl AppContext {
    /// Read-only snapshot of the persisted queue, used by the `!queue` chat
    /// command.
    pub async fn current_queue(&self) -> Vec<QueueItem> {
        self.persisted.read().await.queue.clone()
    }

    pub async fn process_request(
        self: &Arc<Self>,
        requested_by: &str,
        query: &str,
        is_privileged: bool,
        source: &str,
    ) -> CommandResult {
        let query = query.trim();
        let snapshot = self.persisted.read().await.clone();

        if let Err(message) = queue_engine::validate_request(
            &snapshot.queue,
            &snapshot.settings,
            requested_by,
            query,
            is_privileged,
        ) {
            return CommandResult::error(message);
        }

        let mut track = None;
        let mut match_confidence = None;
        match self
            .apple_catalog
            .search_top_track(query, &snapshot.settings.apple_music)
            .await
        {
            Ok(found) => {
                if let Some(found_track) = found {
                    if let Err(message) = queue_engine::ensure_track_allowed(
                        &snapshot.queue,
                        &snapshot.settings,
                        &found_track,
                        is_privileged,
                    ) {
                        return CommandResult::error(message);
                    }
                    match_confidence = Some(queue_engine::estimate_match_confidence(
                        query,
                        &found_track.title,
                        &found_track.artist_name,
                    ));
                    track = Some(found_track);
                } else {
                    self.add_log(
                        LogLevel::Info,
                        format!(
                            "No Apple Music song match was found for \"{query}\". Falling back to manual review."
                        ),
                    )
                    .await;
                }
            }
            Err(error) => {
                self.add_log(
                    LogLevel::Warn,
                    format!("Apple Music lookup failed for \"{query}\": {error}"),
                )
                .await;
                self.nudge_apple_health();
            }
        }

        let request = queue_engine::build_queue_item(
            requested_by,
            query,
            source,
            track.clone(),
            match_confidence,
        );
        {
            // Re-check against the live queue before adding. The checks above
            // ran on a snapshot taken before the catalog lookup awaited the
            // network, and requests from chat, the dashboard and Channel
            // Points can all land in that window — without this, a burst
            // could slip past the per-user and duplicate limits.
            let mut persisted = self.persisted.write().await;
            let live_check = queue_engine::validate_request(
                &persisted.queue,
                &persisted.settings,
                requested_by,
                query,
                is_privileged,
            )
            .and_then(|()| match track.as_ref() {
                Some(found) => queue_engine::ensure_track_allowed(
                    &persisted.queue,
                    &persisted.settings,
                    found,
                    is_privileged,
                ),
                None => Ok(()),
            });
            if let Err(message) = live_check {
                return CommandResult::error(message);
            }
            persisted.queue.push(request.clone());
        }

        let _ = self.save_persisted().await;
        self.emit_state().await;

        if let Some(track) = track {
            self.add_log(
                LogLevel::Info,
                format!("Queued \"{}\" for {}.", track.title, requested_by),
            )
            .await;
            self.ensure_queue_progress("new matched request").await;
            CommandResult::ok(format!("Queued {} by {}.", track.title, track.artist_name))
        } else {
            self.add_log(
                LogLevel::Info,
                format!("Queued manual review request \"{query}\" for {requested_by}."),
            )
            .await;
            CommandResult::ok(format!("Saved \"{query}\" for manual Apple Music review."))
        }
    }

    pub async fn enqueue_manual_request(
        self: &Arc<Self>,
        requested_by: &str,
        query: &str,
    ) -> CommandResult {
        self.process_request(requested_by, query, true, "dashboard")
            .await
    }

    /// Drops a request whose song has started playing. Unlike
    /// `remove_request`, it never touches the player: the song is meant to
    /// play.
    pub(super) async fn retire_playing_request(self: &Arc<Self>, id: &str) {
        {
            let mut persisted = self.persisted.write().await;
            persisted.queue.retain(|item| item.id != id);
        }
        let _ = self.save_persisted().await;
        self.emit_state().await;
    }

    pub async fn remove_request(self: &Arc<Self>, id: &str) -> AppState {
        let removed = {
            let mut persisted = self.persisted.write().await;
            let removed = persisted.queue.iter().find(|item| item.id == id).cloned();
            persisted.queue.retain(|item| item.id != id);
            removed
        };
        if let Some(item) = removed {
            self.withdraw_from_player(&item).await;
        }
        let _ = self.save_persisted().await;
        self.emit_state().await;
        self.ensure_queue_progress("queue removal").await;
        self.snapshot().await
    }

    pub async fn clear_queue(&self) -> AppState {
        let removed = {
            let mut persisted = self.persisted.write().await;
            std::mem::take(&mut persisted.queue)
        };
        // A request already handed to the player sits in Apple Music's
        // Playing Next and would still play unless it's taken back out.
        for item in &removed {
            self.withdraw_from_player(item).await;
        }
        let _ = self.save_persisted().await;
        self.emit_state().await;
        self.snapshot().await
    }

    pub async fn remove_latest_request_by_user(
        self: &Arc<Self>,
        requested_by: &str,
    ) -> CommandResult {
        let removed = {
            let mut persisted = self.persisted.write().await;
            queue_engine::remove_latest_request_by_user(&mut persisted.queue, requested_by)
        };

        match removed {
            Some(item) => {
                self.withdraw_from_player(&item).await;
                let _ = self.save_persisted().await;
                self.emit_state().await;
                self.add_log(
                    LogLevel::Info,
                    format!("Removed the latest request for {requested_by}."),
                )
                .await;
                self.ensure_queue_progress("user remove").await;
                CommandResult::ok("Removed your most recent request.")
            }
            // Nothing waiting, but their own request may be the one playing.
            None if self.is_playing_request_of(requested_by).await => {
                match self.player_bridge.run_command(&self.handle, "skip", None).await {
                    Ok(_) => {
                        self.runtime.write().await.now_playing_request = None;
                        self.add_log(
                            LogLevel::Info,
                            format!("Skipped the playing request for {requested_by} (!remove)."),
                        )
                        .await;
                        CommandResult::ok("Skipped your song.")
                    }
                    Err(error) => CommandResult::error(format!("Couldn't skip your song: {error}")),
                }
            }
            None => CommandResult::error("You do not have any active requests to remove."),
        }
    }

    /// Whether the track playing now is this viewer's request.
    async fn is_playing_request_of(&self, requested_by: &str) -> bool {
        let runtime = self.runtime.read().await;
        overlay::current_request(&runtime)
            .is_some_and(|request| request.requested_by.eq_ignore_ascii_case(requested_by))
    }

    /// A request removed after it was handed to the player also has to leave
    /// the player's own queue, or the song still plays.
    async fn withdraw_from_player(&self, item: &QueueItem) {
        if item.handoff_state != QueueHandoffState::SentToPlayer {
            return;
        }
        let Some(track) = item.track.as_ref() else {
            return;
        };
        let outcome = self.player_bridge.withdraw_track(&self.handle, &track.id).await;
        let note = match outcome {
            queue_engine::UnqueueOutcome::Removed => "taken out of Apple Music's Playing Next",
            queue_engine::UnqueueOutcome::Absent => "it was no longer in Apple Music's queue",
            queue_engine::UnqueueOutcome::Playing => "it was already playing, so it was skipped",
            queue_engine::UnqueueOutcome::Unsupported => "it will be skipped when it starts",
        };
        self.add_log(
            LogLevel::Info,
            format!("Withdrew \"{}\" from the player: {note}.", track.title),
        )
        .await;
    }

    pub async fn search_apple_music(&self, query: &str) -> Result<SearchResult> {
        let settings = self.current_settings().await;
        let matches = self
            .apple_catalog
            .search_tracks(query, &settings.apple_music)
            .await?;
        Ok(SearchResult {
            query: query.to_string(),
            matches,
        })
    }

    pub async fn open_track(&self, payload: OpenTrackPayload) -> CommandResult {
        match self.resolve_track_target(payload).await {
            Ok(target) => match window_shell::open_external(&target) {
                Ok(_) => {
                    self.add_log(
                        LogLevel::Info,
                        format!("Opened Apple Music target: {target}"),
                    )
                    .await;
                    CommandResult::ok("Opened the request in Apple Music.")
                }
                Err(error) => {
                    self.add_log(
                        LogLevel::Error,
                        format!("Open track failed for target {target}: {error}"),
                    )
                    .await;
                    CommandResult::error(error.to_string())
                }
            },
            Err(error) => {
                self.add_log(
                    LogLevel::Error,
                    format!("Open track target resolution failed: {error}"),
                )
                .await;
                CommandResult::error(error.to_string())
            }
        }
    }

    pub(super) async fn update_request_handoff(
        &self,
        request_id: &str,
        state: QueueHandoffState,
        note: Option<String>,
    ) {
        let mut changed = false;
        {
            let mut persisted = self.persisted.write().await;
            if let Some(item) = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == request_id)
            {
                item.handoff_state = state;
                item.handoff_note = note.filter(|value| !value.trim().is_empty());
                item.handoff_updated_at = Some(crate::models::now_iso());
                if matches!(item.handoff_state, QueueHandoffState::SentToPlayer) {
                    item.dispatched_at = item.handoff_updated_at.clone();
                }
                changed = true;
            }
        }

        if changed {
            let _ = self.save_persisted().await;
            self.emit_state().await;
        }
    }

    pub(super) async fn ensure_queue_progress(self: &Arc<Self>, reason: &str) {
        self.promote_front_request(reason).await;

        let request = {
            let persisted = self.persisted.read().await;
            let settings = persisted.settings.clone();
            let Some(request) = persisted.queue.first().cloned() else {
                return;
            };

            if !settings.player.auto_queue
                || request.resolution != crate::models::ResolutionStatus::Matched
                || request.track.is_none()
                || request.requires_manual_review
                || !matches!(
                    request.handoff_state,
                    QueueHandoffState::PendingMatch | QueueHandoffState::ReadyToSend
                )
            {
                return;
            }

            request
        };

        {
            let mut runtime = self.runtime.write().await;
            // While Listen together plays another streamer's music, requests
            // wait in the queue; the next followed song would replace them.
            if runtime.auto_handoff_in_flight || runtime.listen.phase != "off" {
                return;
            }
            runtime.auto_handoff_in_flight = true;
        }

        let track = request.track.as_ref().expect("auto mode requires a track");
        let outcome = self.player_bridge.dispatch_track(&self.handle, &track.id).await;

        let (ok, detail) = match &outcome {
            Ok(summary) => (true, summary.clone()),
            Err(error) => (false, error.to_string()),
        };
        self.add_log(
            if ok { LogLevel::Info } else { LogLevel::Warn },
            format!(
                "Auto-queue dispatch for \"{}\" after {}: {}",
                track.title, reason, detail
            ),
        )
        .await;

        self.update_request_handoff(
            &request.id,
            if ok {
                QueueHandoffState::SentToPlayer
            } else {
                QueueHandoffState::FailedDispatch
            },
            Some(detail),
        )
        .await;

        let mut runtime = self.runtime.write().await;
        runtime.auto_handoff_in_flight = false;
    }

    /// Keeps the front request's manual-review flag in sync. A matched
    /// request with no manual review simply stays PendingMatch until it is
    /// dispatched (auto-queue or the manual "send now" action); there is no
    /// separate ReadyToSend staging step to promote it into.
    pub(super) async fn promote_front_request(&self, _reason: &str) {
        let mut changed = false;
        {
            let mut persisted = self.persisted.write().await;
            let Some(front) = persisted.queue.first_mut() else {
                return;
            };

            if (front.track.is_none() || front.resolution == ResolutionStatus::ManualReview)
                && (front.handoff_state != QueueHandoffState::ManualReview
                    || !front.requires_manual_review)
            {
                front.handoff_state = QueueHandoffState::ManualReview;
                front.requires_manual_review = true;
                front.handoff_note =
                    Some("Manual review required before this request can be sent.".to_string());
                front.handoff_updated_at = Some(crate::models::now_iso());
                changed = true;
            }
        }

        if changed {
            let _ = self.save_persisted().await;
            self.emit_state().await;
        }
    }

    pub async fn dispatch_next_request(self: &Arc<Self>) -> Result<AppState> {
        self.dispatch_request_inner("dashboard dispatch").await?;
        Ok(self.snapshot().await)
    }

    pub(super) async fn dispatch_request_inner(self: &Arc<Self>, source: &str) -> Result<()> {
        let request = {
            let persisted = self.persisted.read().await;
            let Some(request) = persisted.queue.first().cloned() else {
                anyhow::bail!("No request is ready to dispatch.");
            };

            if request.track.is_none() || request.requires_manual_review {
                anyhow::bail!("The front request still needs a matched Apple Music track.");
            }

            if !matches!(
                request.handoff_state,
                QueueHandoffState::PendingMatch
                    | QueueHandoffState::ReadyToSend
                    | QueueHandoffState::FailedDispatch
            ) {
                anyhow::bail!("The front request cannot be dispatched right now.");
            }

            request
        };

        let track = request.track.as_ref().expect("dispatch requires a track");
        let outcome = self.player_bridge.dispatch_track(&self.handle, &track.id).await;

        let (ok, detail) = match &outcome {
            Ok(summary) => (true, summary.clone()),
            Err(error) => (false, error.to_string()),
        };
        self.add_log(
            if ok { LogLevel::Info } else { LogLevel::Warn },
            format!(
                "Player dispatch for \"{}\" via {}: {}",
                track.title, source, detail
            ),
        )
        .await;

        let handoff_note = if ok {
            Some(format!("Triggered via {source}: {detail}"))
        } else {
            Some(detail.clone())
        };
        self.update_request_handoff(
            &request.id,
            if ok {
                QueueHandoffState::SentToPlayer
            } else {
                QueueHandoffState::FailedDispatch
            },
            handoff_note,
        )
        .await;

        if ok {
            Ok(())
        } else {
            anyhow::bail!(detail)
        }
    }

    pub async fn approve_request(
        self: &Arc<Self>,
        payload: ApproveRequestPayload,
    ) -> Result<AppState> {
        let target_id = if let Some(request_id) = payload.request_id.clone() {
            request_id
        } else {
            self.persisted
                .read()
                .await
                .queue
                .first()
                .map(|item| item.id.clone())
                .ok_or_else(|| anyhow!("No request is available to approve."))?
        };

        {
            let mut persisted = self.persisted.write().await;
            let item = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == target_id)
                .ok_or_else(|| anyhow!("The selected request no longer exists."))?;

            if let Some(track) = payload.track.clone() {
                item.track = Some(track.clone());
                item.resolution = ResolutionStatus::Matched;
                item.resolved_track_url = Some(track.url.clone());
                item.match_confidence = Some(queue_engine::estimate_match_confidence(
                    &item.query,
                    &track.title,
                    &track.artist_name,
                ));
            }

            if item.track.is_none() {
                anyhow::bail!("Approve requires a matched Apple Music track.");
            }

            item.requires_manual_review = false;
            item.handoff_state = QueueHandoffState::ReadyToSend;
            item.handoff_note = Some("Matched and ready to dispatch into Apple Music.".to_string());
            item.handoff_updated_at = Some(crate::models::now_iso());
        }

        self.save_persisted().await?;
        self.emit_state().await;
        self.ensure_queue_progress("request approved").await;
        Ok(self.snapshot().await)
    }

    pub async fn send_request_to_manual_review(self: &Arc<Self>, id: &str) -> Result<AppState> {
        {
            let mut persisted = self.persisted.write().await;
            let item = persisted
                .queue
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| anyhow!("The selected request no longer exists."))?;
            item.resolution = ResolutionStatus::ManualReview;
            item.requires_manual_review = true;
            item.handoff_state = QueueHandoffState::ManualReview;
            item.handoff_note =
                Some("Manual review requested before this can be sent.".to_string());
            item.handoff_updated_at = Some(crate::models::now_iso());
        }

        self.save_persisted().await?;
        self.emit_state().await;
        Ok(self.snapshot().await)
    }

    pub(super) async fn resolve_track_target(&self, payload: OpenTrackPayload) -> Result<String> {
        if let Some(url) = payload.url.filter(|value| !value.trim().is_empty()) {
            return Ok(url);
        }

        if let Some(request) = self.find_request(payload.request_id.as_deref()).await {
            if let Some(track) = request.track {
                return Ok(track.url);
            }

            let settings = self.current_settings().await;
            return Ok(AppleCatalog::build_search_url(
                &request.query,
                &settings.apple_music.storefront,
            ));
        }

        if let Some(query) = payload.query.filter(|value| !value.trim().is_empty()) {
            let settings = self.current_settings().await;
            return Ok(AppleCatalog::build_search_url(
                &query,
                &settings.apple_music.storefront,
            ));
        }

        Err(anyhow!("No Apple Music target was available to open."))
    }

    pub async fn find_request(&self, request_id: Option<&str>) -> Option<QueueItem> {
        let persisted = self.persisted.read().await;
        match request_id {
            Some(id) => persisted.queue.iter().find(|item| item.id == id).cloned(),
            None => persisted.queue.first().cloned(),
        }
    }
}
