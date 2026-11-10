use crate::api::dashboard::{is_authenticated, redirect_msg};
use crate::api::state::AppState;
use crate::domain::telegram::TelegramSettings;
use crate::infra::db::now_timestamp;
use crate::infra::telegram_client::TelegramClient;
use crate::infra::telegram_db::save_telegram_settings;
use axum::{
    extract::{Form, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TelegramSettingsForm {
    pub bot_token: Option<String>,
    pub chat_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<String>,
}

pub async fn update_telegram_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<TelegramSettingsForm>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let existing = state.telegram.current_settings().await;
    let new_token = form
        .bot_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(existing.bot_token);

    let new_chat_id = form
        .chat_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(existing.chat_id);

    let is_enabled =
        form.enabled.as_deref() == Some("on") || form.enabled.as_deref() == Some("true");

    let updated = TelegramSettings {
        bot_token: new_token,
        chat_id: new_chat_id,
        enabled: is_enabled,
        updated_at: now_timestamp(),
    };

    if let Err(e) = save_telegram_settings(&state.db, &updated).await {
        return redirect_msg(&format!("Failed saving Telegram settings: {e:#}"), false);
    }

    if updated.is_configured() {
        let token = updated.bot_token.clone();
        tokio::spawn(async move {
            let client = TelegramClient::new();
            let commands = crate::domain::telegram::default_bot_commands();
            if let Err(e) = client.set_my_commands(&token, commands).await {
                tracing::warn!("Failed registering Telegram bot commands on update: {e:#}");
            }
        });
    }

    state.telegram.update_settings(updated).await;
    redirect_msg("Telegram settings updated successfully", true)
}

pub async fn test_telegram_notification(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let settings = state.telegram.current_settings().await;
    if !settings.is_configured() {
        return redirect_msg(
            "Telegram is not configured or disabled. Fill in Token & Chat ID first.",
            false,
        );
    }

    let client = TelegramClient::new();
    let test_msg = format!(
        "🔔 *DeeperSeeker: Test Notification*\n\n\
         • Gateway Port: `:{}`\n\
         • Version: `v{}`\n\
         • Status: *Active & Operational*\n\n\
         Your Telegram bot notifications are properly configured!",
        state.config.port,
        env!("CARGO_PKG_VERSION")
    );

    match client
        .send_message(&settings.bot_token, &settings.chat_id, &test_msg)
        .await
    {
        Ok(()) => redirect_msg(
            "Telegram test message sent successfully! Check your chat.",
            true,
        ),
        Err(e) => redirect_msg(&format!("Telegram test message failed: {e:#}"), false),
    }
}
