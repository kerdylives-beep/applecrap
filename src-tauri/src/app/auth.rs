//! Sign in with Twitch: the device-code flow, token refresh, and the hourly
//! validation Twitch requires.

use crate::{
    models::{AuthSlot, AuthSummary, PendingSignIn, SignedInAccount, TwitchAuth},
    services::twitch_auth::{self, RefreshError, TokenPoll},
};

use super::*;

/// Refresh a token this long before it expires, so a request never races
/// the expiry.
const REFRESH_MARGIN_SECS: i64 = 5 * 60;

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

impl AppContext {
    /// Starts signing in `slot`: asks Twitch for a code the user enters at
    /// twitch.tv/activate, then polls in the background until they do.
    pub async fn begin_sign_in(self: &Arc<Self>, slot: AuthSlot) -> Result<AppState> {
        let client_id = twitch_auth::client_id()
            .ok_or_else(|| anyhow!("Sign in with Twitch isn't set up in this build."))?;
        self.cancel_sign_in_task().await;

        let scopes = twitch_auth::scopes_for(slot);
        let device = twitch_auth::request_device_code(&self.http, client_id, scopes).await?;

        {
            let mut runtime = self.runtime.write().await;
            runtime.auth_error = None;
            runtime.pending_sign_in = Some(PendingSignIn {
                slot,
                user_code: device.user_code.clone(),
                verification_uri: device.verification_uri.clone(),
                expires_at: now_secs() + device.expires_in,
            });
        }

        let poller = Arc::clone(self);
        let task = tauri::async_runtime::spawn(async move {
            poller.poll_sign_in(slot, client_id, scopes, device).await;
        });
        *self.sign_in_task.lock().await = Some(task);

        self.emit_state().await;
        Ok(self.snapshot().await)
    }

    async fn poll_sign_in(
        self: Arc<Self>,
        slot: AuthSlot,
        client_id: &'static str,
        scopes: &'static str,
        device: twitch_auth::DeviceAuthorization,
    ) {
        let mut interval = Duration::from_secs(device.interval.max(1));
        let deadline = now_secs() + device.expires_in;

        loop {
            tokio::time::sleep(interval).await;
            if now_secs() > deadline {
                self.fail_sign_in("The sign-in code expired. Start again when you're ready.")
                    .await;
                return;
            }

            match twitch_auth::poll_device_token(&self.http, client_id, scopes, &device.device_code)
                .await
            {
                Ok(TokenPoll::Pending) => {}
                Ok(TokenPoll::SlowDown) => interval += Duration::from_secs(5),
                Ok(TokenPoll::Granted(grant)) => {
                    if let Err(error) = self.complete_sign_in(slot, grant).await {
                        self.fail_sign_in(&format!("Signing in didn't finish: {error}"))
                            .await;
                    }
                    return;
                }
                Ok(TokenPoll::Expired) => {
                    self.fail_sign_in("The sign-in code expired. Start again when you're ready.")
                        .await;
                    return;
                }
                Ok(TokenPoll::Failed(message)) => {
                    self.fail_sign_in(&message).await;
                    return;
                }
                // A network blip mid-poll: keep trying until the code expires.
                Err(_) => {}
            }
        }
    }

    async fn complete_sign_in(
        self: &Arc<Self>,
        slot: AuthSlot,
        grant: twitch_auth::TokenGrant,
    ) -> Result<()> {
        let info = twitch_auth::validate(&self.http, &grant.access_token)
            .await?
            .ok_or_else(|| anyhow!("Twitch did not accept the new sign-in"))?;
        let login = info.login.clone();
        let reconnect_chat;

        {
            let mut persisted = self.persisted.write().await;
            *persisted.auth.slot_mut(slot) = Some(TwitchAuth {
                login: info.login,
                user_id: info.user_id,
                access_token: grant.access_token,
                refresh_token: grant.refresh_token,
                expires_at: now_secs() + grant.expires_in,
                scopes: if grant.scope.is_empty() { info.scopes } else { grant.scope },
            });
            if slot == AuthSlot::Broadcaster && persisted.settings.twitch.channel.trim().is_empty() {
                persisted.settings.twitch.channel = login.clone();
            }
        }
        {
            let mut runtime = self.runtime.write().await;
            runtime.pending_sign_in = None;
            runtime.auth_error = None;
            reconnect_chat = runtime.bot_status.connected
                || matches!(runtime.bot_status.state, BotConnectionState::Connecting);
        }

        self.save_persisted().await?;
        self.add_log(
            LogLevel::Info,
            format!("Signed in to Twitch as @{login} ({}).", slot.label()),
        )
        .await;
        self.emit_state().await;

        // A new chat identity only takes effect on a fresh connection.
        if reconnect_chat {
            let _ = self.connect_bot().await;
        }
        Ok(())
    }

    async fn fail_sign_in(&self, message: &str) {
        {
            let mut runtime = self.runtime.write().await;
            runtime.pending_sign_in = None;
            runtime.auth_error = Some(message.to_string());
        }
        self.add_log(LogLevel::Warn, format!("Twitch sign-in failed: {message}"))
            .await;
        self.emit_state().await;
    }

    pub async fn cancel_sign_in(&self) -> AppState {
        self.cancel_sign_in_task().await;
        self.runtime.write().await.pending_sign_in = None;
        self.emit_state().await;
        self.snapshot().await
    }

    async fn cancel_sign_in_task(&self) {
        if let Some(task) = self.sign_in_task.lock().await.take() {
            task.abort();
        }
    }

    pub async fn sign_out(self: &Arc<Self>, slot: AuthSlot) -> Result<AppState> {
        let removed = {
            let _turn = self.auth_refresh_lock.lock().await;
            self.persisted.write().await.auth.slot_mut(slot).take()
        };
        self.save_persisted().await?;

        if let Some(account) = removed.as_ref() {
            // Best effort: the local copy is already gone either way.
            if let Some(client_id) = twitch_auth::client_id() {
                let http = self.http.clone();
                let token = account.access_token.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = twitch_auth::revoke(&http, client_id, &token).await;
                });
            }
            self.add_log(
                LogLevel::Info,
                format!("Signed out @{} ({}).", account.login, slot.label()),
            )
            .await;
        }

        let chat_was_live = {
            let runtime = self.runtime.read().await;
            runtime.bot_status.connected
                || matches!(runtime.bot_status.state, BotConnectionState::Connecting)
        };
        if chat_was_live {
            // Reconnect with whichever identity remains, or stop cleanly.
            if self.chat_login().await.is_ok() {
                let _ = self.connect_bot().await;
            } else {
                let _ = self.disconnect_bot().await;
            }
        }
        self.emit_state().await;
        Ok(self.snapshot().await)
    }

    /// A valid access token for `slot`, refreshed first if it is close to
    /// expiring. Refreshes are serialized: Twitch refresh tokens are one-time
    /// use, so two concurrent refreshes would sign the user out.
    pub async fn access_token(&self, slot: AuthSlot) -> Result<String> {
        let _turn = self.auth_refresh_lock.lock().await;
        let current = self
            .persisted
            .read()
            .await
            .auth
            .slot(slot)
            .cloned()
            .ok_or_else(|| anyhow!("The {} account isn't signed in.", slot.label()))?;
        if current.expires_at - now_secs() > REFRESH_MARGIN_SECS {
            return Ok(current.access_token);
        }
        self.refresh_locked(slot, &current).await
    }

    /// Refresh after Twitch rejected `stale_token` — unless another caller
    /// already refreshed it, in which case the current token is returned.
    pub async fn refresh_after_rejection(&self, slot: AuthSlot, stale_token: &str) -> Result<String> {
        let _turn = self.auth_refresh_lock.lock().await;
        let current = self
            .persisted
            .read()
            .await
            .auth
            .slot(slot)
            .cloned()
            .ok_or_else(|| anyhow!("The {} account isn't signed in.", slot.label()))?;
        if current.access_token != stale_token {
            return Ok(current.access_token);
        }
        self.refresh_locked(slot, &current).await
    }

    /// Caller must hold `auth_refresh_lock`.
    async fn refresh_locked(&self, slot: AuthSlot, current: &TwitchAuth) -> Result<String> {
        let client_id = twitch_auth::client_id()
            .ok_or_else(|| anyhow!("Sign in with Twitch isn't set up in this build."))?;

        match twitch_auth::refresh(&self.http, client_id, &current.refresh_token).await {
            Ok(grant) => {
                let access = grant.access_token.clone();
                if let Some(account) = self.persisted.write().await.auth.slot_mut(slot).as_mut() {
                    account.access_token = grant.access_token;
                    // One-time use: the new refresh token must be saved now.
                    account.refresh_token = grant.refresh_token;
                    account.expires_at = now_secs() + grant.expires_in;
                    if !grant.scope.is_empty() {
                        account.scopes = grant.scope;
                    }
                }
                self.save_persisted().await?;
                Ok(access)
            }
            Err(RefreshError::Invalid) => {
                *self.persisted.write().await.auth.slot_mut(slot) = None;
                let message = format!(
                    "Your Twitch sign-in for @{} expired. Sign in again to keep the {} connected.",
                    current.login,
                    slot.label()
                );
                self.runtime.write().await.auth_error = Some(message.clone());
                let _ = self.save_persisted().await;
                self.add_log(LogLevel::Warn, message.clone()).await;
                self.emit_state().await;
                Err(anyhow!(message))
            }
            Err(RefreshError::Transient(error)) => Err(error),
        }
    }

    /// Twitch requires validating tokens at startup and hourly thereafter.
    /// A token Twitch no longer accepts is refreshed; if that fails the
    /// account is signed out and the user is told.
    pub(super) async fn validate_signed_in_accounts(&self) {
        for slot in [AuthSlot::Bot, AuthSlot::Broadcaster] {
            let token = self
                .persisted
                .read()
                .await
                .auth
                .slot(slot)
                .map(|account| account.access_token.clone());
            let Some(token) = token else {
                continue;
            };
            match twitch_auth::validate(&self.http, &token).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = self.refresh_after_rejection(slot, &token).await;
                }
                // Offline: try again next hour rather than signing anyone out.
                Err(_) => {}
            }
        }
    }

    pub(super) fn auth_summary(&self, persisted: &PersistedState, runtime: &RuntimeState) -> AuthSummary {
        let account = |slot: AuthSlot| {
            persisted.auth.slot(slot).map(|auth| SignedInAccount {
                login: auth.login.clone(),
                can_manage_rewards: auth
                    .scopes
                    .iter()
                    .any(|scope| scope == "channel:manage:redemptions"),
            })
        };
        AuthSummary {
            available: twitch_auth::client_id().is_some(),
            bot: account(AuthSlot::Bot),
            broadcaster: account(AuthSlot::Broadcaster),
            pending: runtime.pending_sign_in.clone(),
            error: runtime.auth_error.clone(),
        }
    }

    /// Opens Twitch's activation page for the pending sign-in.
    pub async fn open_sign_in_page(&self) -> CommandResult {
        let Some(pending) = self.runtime.read().await.pending_sign_in.clone() else {
            return CommandResult::error("There's no sign-in waiting for a code.");
        };
        if !twitch_auth::is_twitch_activation_url(&pending.verification_uri) {
            return CommandResult::error("Twitch sent an unexpected sign-in address.");
        }
        match window_shell::open_twitch_activation(&pending.verification_uri) {
            Ok(()) => CommandResult::ok("Opened Twitch in your browser."),
            Err(error) => CommandResult::error(error.to_string()),
        }
    }
}
