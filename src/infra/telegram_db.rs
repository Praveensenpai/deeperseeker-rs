use crate::domain::telegram::TelegramSettings;
use crate::infra::db::now_timestamp;
use anyhow::{Context, Result};
use rusqlite::params;
use tokio_rusqlite::Connection;

pub async fn get_telegram_settings(conn: &Connection) -> Result<TelegramSettings> {
    let settings = conn
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT bot_token, chat_id, enabled, updated_at \
                 FROM telegram_settings WHERE id = 1",
            )?;
            let mut rows = stmt.query([])?;
            if let Some(row) = rows.next()? {
                let enabled_num: i64 = row.get(2)?;
                Ok(TelegramSettings {
                    bot_token: row.get(0)?,
                    chat_id: row.get(1)?,
                    enabled: enabled_num != 0,
                    updated_at: row.get(3)?,
                })
            } else {
                Ok(TelegramSettings::default())
            }
        })
        .await
        .context("Failed to load telegram settings from db")?;

    // If unconfigured in DB, inspect environment variables as initial defaults
    if settings.bot_token.is_empty() && settings.chat_id.is_empty() {
        let env_token = std::env::var("DEEPSEEKER_TELEGRAM_BOT_TOKEN").unwrap_or_default();
        let env_chat = std::env::var("DEEPSEEKER_TELEGRAM_CHAT_ID").unwrap_or_default();
        if !env_token.is_empty() && !env_chat.is_empty() {
            return Ok(TelegramSettings {
                bot_token: env_token,
                chat_id: env_chat,
                enabled: true,
                updated_at: now_timestamp(),
            });
        }
    }

    Ok(settings)
}

pub async fn save_telegram_settings(conn: &Connection, settings: &TelegramSettings) -> Result<()> {
    let bot_token = settings.bot_token.clone();
    let chat_id = settings.chat_id.clone();
    let enabled_num = if settings.enabled { 1i64 } else { 0i64 };
    let now = now_timestamp();

    conn.call(move |c| {
        c.execute(
            "INSERT INTO telegram_settings (id, bot_token, chat_id, enabled, updated_at) \
             VALUES (1, ?1, ?2, ?3, ?4) \
             ON CONFLICT(id) DO UPDATE SET \
                bot_token = excluded.bot_token, \
                chat_id = excluded.chat_id, \
                enabled = excluded.enabled, \
                updated_at = excluded.updated_at",
            params![bot_token, chat_id, enabled_num, now],
        )?;
        Ok(())
    })
    .await
    .context("Failed to save telegram settings to db")?;

    Ok(())
}
