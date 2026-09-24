//! The Twitch chat connection and the identity it logs in with.

use super::*;

impl AppContext {
    pub async fn connect_bot(self: &Arc<Self>) -> Result<AppState> {
        twitch_service::connect(Arc::clone(self)).await?;
        Ok(self.snapshot().await)
    }

    pub async fn disconnect_bot(self: &Arc<Self>) -> Result<AppState> {
        twitch_service::disconnect(Arc::clone(self)).await?;
        Ok(self.snapshot().await)
    }

    pub async fn register_twitch_connection(
        &self,
        id: u64,
        writer: mpsc::UnboundedSender<twitch_service::Outbound>,
        task: JoinHandle<()>,
    ) {
        let mut connection = self.twitch_connection.lock().await;
        *connection = Some(TwitchConnection { id, writer, task });
    }

    pub async fn abort_twitch_connection(&self) {
        let mut connection = self.twitch_connection.lock().await;
        if let Some(existing) = connection.take() {
            let _ = existing
                .writer
                .send(twitch_service::Outbound::Raw("QUIT :Disconnecting\r\n".to_string()));
            existing.task.abort();
        }
    }

    /// Posts a message to the connected channel (rate-limited with the bot's
    /// other replies). A no-op when the bot is offline.
    pub async fn send_chat(&self, text: impl Into<String>) {
        if let Some(connection) = self.twitch_connection.lock().await.as_ref() {
            let _ = connection
                .writer
                .send(twitch_service::Outbound::chat(text));
        }
    }

    /// The signed-in account chat uses: the bot account first, then the
    /// broadcaster. `None` means a pasted legacy token (if any) is used.
    async fn chat_account(&self) -> Option<(AuthSlot, String)> {
        let persisted = self.persisted.read().await;
        [AuthSlot::Bot, AuthSlot::Broadcaster]
            .into_iter()
            .find_map(|slot| persisted.auth.slot(slot).map(|account| (slot, account.login.clone())))
    }

    /// The login chat will use, without touching the network. Lets a connect
    /// fail fast on missing setup while token refreshes happen (and retry)
    /// inside the connection supervisor.
    pub async fn chat_login(&self) -> Result<String> {
        if let Some((_, login)) = self.chat_account().await {
            return Ok(login);
        }
        let settings = self.current_settings().await;
        let login = settings.twitch.bot_username.trim().to_lowercase();
        if login.is_empty() || settings.twitch.oauth_token.trim().is_empty() {
            anyhow::bail!("Sign in with Twitch, or enter a bot username and token.");
        }
        Ok(login)
    }

    /// Who the bot logs into chat as: a signed-in bot account first, then the
    /// signed-in broadcaster, then a pasted legacy token.
    pub async fn chat_identity(&self) -> Result<twitch_service::ChatIdentity> {
        if let Some((slot, login)) = self.chat_account().await {
            let token = self.access_token(slot).await?;
            return Ok(twitch_service::ChatIdentity {
                login,
                password: format!("oauth:{token}"),
                slot: Some(slot),
            });
        }

        let settings = self.current_settings().await;
        let login = settings.twitch.bot_username.trim().to_lowercase();
        let password = crate::models::normalize_twitch_oauth_token(&settings.twitch.oauth_token);
        if login.is_empty() || password.is_empty() {
            anyhow::bail!("Sign in with Twitch, or enter a bot username and token.");
        }
        Ok(twitch_service::ChatIdentity {
            login,
            password,
            slot: None,
        })
    }

    /// Refreshes the chat token after Twitch rejected it at login.
    pub async fn force_refresh_chat_token(
        &self,
        identity: &twitch_service::ChatIdentity,
    ) -> Result<()> {
        let Some(slot) = identity.slot else {
            anyhow::bail!("This token cannot be refreshed automatically.");
        };
        let stale = identity.password.trim_start_matches("oauth:");
        self.refresh_after_rejection(slot, stale).await.map(|_| ())
    }

    pub async fn update_bot_status(
        &self,
        state: BotConnectionState,
        status: impl Into<String>,
        detail: impl Into<String>,
        channel: Option<String>,
    ) {
        {
            let mut runtime = self.runtime.write().await;
            let connected = matches!(&state, BotConnectionState::Connected);
            let previous_channel = runtime.bot_status.channel.clone();
            runtime.bot_status = BotStatus {
                connected,
                state,
                status: status.into(),
                detail: detail.into(),
                channel: channel.unwrap_or(previous_channel),
                last_event_at: Some(crate::models::now_iso()),
            };
        }
        self.emit_state().await;
    }

    /// Clears the registered connection, but only if it is still the one
    /// identified by `id` — a supervisor stopping late must not clear a newer
    /// connection the user started in the meantime.
    pub async fn clear_twitch_connection(&self, id: u64) {
        let mut connection = self.twitch_connection.lock().await;
        if connection.as_ref().map(|existing| existing.id) == Some(id) {
            *connection = None;
        }
    }
}
