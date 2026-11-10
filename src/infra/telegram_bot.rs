use crate::api::state::AppState;
use crate::domain::usage::format_metric;
use crate::infra::db::{get_tokens, now_timestamp};
use crate::infra::telegram_client::{TelegramClient, TelegramUpdate};
use crate::infra::usage_db::{get_all_summaries, get_token_usages};
use std::collections::HashMap;
use std::time::Duration;

pub fn start_telegram_bot_poller(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = TelegramClient::new();
        let mut offset = 0i64;
        let mut registered_token: Option<String> = None;

        loop {
            let settings = state.telegram.current_settings().await;
            if !settings.is_configured() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            if registered_token.as_deref() != Some(&settings.bot_token) {
                ensure_bot_commands_registered(
                    &client,
                    &settings.bot_token,
                    &mut registered_token,
                )
                .await;
            }

            match client.get_updates(&settings.bot_token, offset, 25).await {
                Ok(updates) => {
                    for update in updates {
                        if update.update_id >= offset {
                            offset = update.update_id + 1;
                        }
                        handle_update(
                            &state,
                            &client,
                            &settings.bot_token,
                            &settings.chat_id,
                            update,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    tracing::debug!("Telegram bot polling tick error: {e:#}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    })
}

async fn ensure_bot_commands_registered(
    client: &TelegramClient,
    bot_token: &str,
    registered_token: &mut Option<String>,
) {
    let commands = crate::domain::telegram::default_bot_commands();
    match client.set_my_commands(bot_token, commands).await {
        Ok(_) => {
            tracing::info!("Registered Telegram bot commands with Telegram API");
            *registered_token = Some(bot_token.to_string());
        }
        Err(e) => {
            tracing::warn!("Failed registering Telegram bot commands: {e:#}");
        }
    }
}

async fn handle_update(
    state: &AppState,
    client: &TelegramClient,
    bot_token: &str,
    configured_chat_id: &str,
    update: TelegramUpdate,
) {
    let Some(msg) = update.message else { return };
    let chat_id_str = msg.chat.id.to_string();

    // Security boundary: Only respond to authorized chat ID
    if chat_id_str != configured_chat_id {
        tracing::warn!(
            "Unauthorized Telegram interaction attempt from chat_id: {}",
            chat_id_str
        );
        return;
    }

    let text = msg.text.as_deref().unwrap_or("").trim();
    if text.is_empty() {
        return;
    }

    let (cmd, args) = match text.split_once(char::is_whitespace) {
        Some((c, a)) => (c.trim(), a.trim()),
        None => (text.trim(), ""),
    };

    let reply = match cmd {
        "/start" | "/help" => build_help_message(),
        "/status" => build_status_message(state).await,
        "/tokens" => build_tokens_message(state).await,
        "/usage" => build_usage_message(state).await,
        "/ping" => "🏓 *Pong!* DeeperSeeker gateway is online and operational.".to_string(),
        "/add" => handle_add_token_command(state, args).await,
        "/del" | "/remove" => handle_del_token_command(state, args).await,
        _ => format!(
            "❓ Unknown command `{cmd}`.\n\n\
             Available commands:\n\
             • `/add <token> [alias]` - Add verified upstream token\n\
             • `/del <id>` - Remove token from pool\n\
             • `/status` - Gateway & token pool health\n\
             • `/tokens` - Detailed token list\n\
             • `/usage` - Aggregated usage metrics\n\
             • `/ping` - Health check"
        ),
    };

    if let Err(e) = client
        .send_message(bot_token, configured_chat_id, &reply)
        .await
    {
        tracing::warn!("Failed sending Telegram reply: {e:#}");
    }
}

fn build_help_message() -> String {
    format!(
        "🤖 *DeeperSeeker Gateway Bot*\n\n\
         You can manage and monitor your DeepSeek token pool directly from Telegram:\n\n\
         • `/add <token> [alias]` - Verify & add a new DeepSeek auth token\n\
         • `/del <id>` - Remove a token from the pool by ID\n\
         • `/status` - Live pool health, in-flight streams & today's cache rate\n\
         • `/tokens` - Detailed list of upstream tokens with status indicators\n\
         • `/usage` - Aggregated token consumption analytics\n\
         • `/ping` - Connectivity and heartbeat check\n\n\
         _Version: v{}_",
        env!("CARGO_PKG_VERSION")
    )
}

async fn handle_add_token_command(state: &AppState, args: &str) -> String {
    let mut parts = args.split_whitespace();
    let Some(raw_token) = parts.next() else {
        return "⚠️ *Usage:* `/add <TOKEN> [ALIAS]`\n\nExample:\n`/add eyJhbGci... personal`"
            .to_string();
    };

    let token = raw_token.trim().trim_matches('"').trim_matches('\'');
    if token.is_empty() {
        return "⚠️ Token cannot be empty.".to_string();
    }

    let alias = parts.next().map(|s| s.trim().to_string());

    if !crate::api::token_admin::verify_upstream(state, token).await {
        return "❌ *Upstream Verification Failed*\n\nDeepSeek rejected this token. It was not added to the pool.".to_string();
    }

    if let Err(e) = crate::infra::db::add_token(&state.db, token, alias.as_deref()).await {
        return format!("❌ Failed to save token to database: {e:#}");
    }

    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let total_count = tokens.len();
    let active_count = tokens.iter().filter(|t| t.is_active()).count();
    let added = tokens.into_iter().rfind(|t| t.token == token);

    match added {
        Some(tok) => {
            let name = tok.alias.as_deref().unwrap_or("unnamed");
            let masked = tok.masked_token();
            format!(
                "✅ *Token Added & Verified Successfully*\n\n\
                 • *ID:* `#{}`\n\
                 • *Alias:* `{name}`\n\
                 • *Status:* `ACTIVE`\n\
                 • *Key:* `{masked}`\n\
                 • *Pool Health:* `{active_count} active / {total_count} total`",
                tok.id
            )
        }
        None => format!("✅ Token added. Current pool size: {total_count}"),
    }
}

async fn handle_del_token_command(state: &AppState, args: &str) -> String {
    let Some(id_str) = args.split_whitespace().next() else {
        return "⚠️ *Usage:* `/del <TOKEN_ID>`\n\nExample:\n`/del 2`".to_string();
    };

    let Ok(id) = id_str.trim_start_matches('#').parse::<i64>() else {
        return format!("⚠️ Invalid token ID `{id_str}`. Must be an integer (e.g. `/del 2`).");
    };

    let existing = crate::infra::db::get_token(&state.db, id)
        .await
        .unwrap_or(None);
    let Some(tok) = existing else {
        return format!("⚠️ Token `#{id}` not found in pool.");
    };

    if let Err(e) = crate::infra::db::delete_token(&state.db, id).await {
        return format!("❌ Failed to delete token `#{id}`: {e:#}");
    }

    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let active_count = tokens.iter().filter(|t| t.is_active()).count();
    let name = tok.alias.as_deref().unwrap_or("unnamed");

    format!(
        "🗑️ *Token #{} Deleted*\n\n\
         Removed `{name}` from pool.\n\
         Pool now has `{active_count} active / {} total`.",
        id,
        tokens.len()
    )
}

async fn build_status_message(state: &AppState) -> String {
    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let total = tokens.len();
    let active = tokens.iter().filter(|t| t.is_active()).count();
    let limited = tokens.iter().filter(|t| t.is_rate_limited()).count();
    let expired = tokens.iter().filter(|t| t.is_expired()).count();

    let in_flight_map = state.in_flight.lock().await;
    let in_flight: usize = in_flight_map.values().sum();

    let summaries = get_all_summaries(&state.db).await.unwrap_or_default();
    let today = summaries.iter().find(|s| s.period == "Today");

    let (reqs, tokens_str, cache_rate) = match today {
        Some(s) => (
            format_metric(s.requests, false),
            format_metric(s.total_tokens, false),
            format!("{:.1}%", s.cache_hit_rate()),
        ),
        None => ("0".to_string(), "0".to_string(), "0.0%".to_string()),
    };

    format!(
        "📊 *DeeperSeeker Gateway Status*\n\n\
         • *Token Pool:* `{active} active / {total} total`\n\
         • *In-Flight Streams:* `{in_flight}`\n\
         • *Rate Limited:* `{limited}` | *Expired:* `{expired}`\n\
         • *Today's Requests:* `{reqs}`\n\
         • *Today's Tokens:* `{tokens_str}`\n\
         • *Today's Cache Hit Rate:* `{cache_rate}`\n\
         • *Gateway Port:* `:{}`\n\
         • *Version:* `v{}`",
        state.config.port,
        env!("CARGO_PKG_VERSION")
    )
}

fn format_time_ago(last_used: Option<f64>, now: f64) -> String {
    let Some(t) = last_used.filter(|&v| v > 0.0) else {
        return "never".to_string();
    };
    let s = (now - t).max(0.0) as u64;
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h ago", s / 3600)
    }
}

async fn build_tokens_message(state: &AppState) -> String {
    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    if tokens.is_empty() {
        return "⚠️ *Token Pool Empty*: No upstream tokens registered in the gateway.".to_string();
    }

    let usages = get_token_usages(&state.db).await.unwrap_or_default();
    let usage_map: HashMap<i64, _> = usages.iter().map(|u| (u.token_id, u)).collect();
    let now = now_timestamp();

    let mut out = format!("🔑 *Upstream Token Pool ({})*\n\n", tokens.len());
    for tok in tokens {
        let dot = if tok.is_active() {
            "🟢"
        } else if tok.is_rate_limited() {
            "🟡"
        } else {
            "🔴"
        };

        let alias = tok.alias.as_deref().unwrap_or("unnamed");
        let masked = tok.masked_token();
        let last_ago = format_time_ago(tok.last_used, now);

        let usage_str = match usage_map.get(&tok.id) {
            Some(u) => format!(
                "reqs: {} | tot: {}",
                u.requests,
                format_metric(u.total_tokens, false)
            ),
            None => "reqs: 0 | tot: 0".to_string(),
        };

        out.push_str(&format!(
            "{dot} *#{}* `{alias}` ({})\n  • `{masked}`\n  • Used: {last_ago} | {usage_str}\n\n",
            tok.id, tok.status
        ));
    }

    out
}

async fn build_usage_message(state: &AppState) -> String {
    let summaries = get_all_summaries(&state.db).await.unwrap_or_default();
    if summaries.is_empty() {
        return "ℹ️ *No usage records yet.*".to_string();
    }

    let mut out = "📈 *Token Usage Analytics*\n\n".to_string();
    for s in summaries {
        out.push_str(&format!(
            "• *{}*\n  Reqs: `{}` | In: `{}` | Out: `{}`\n  Total: `{}` | Cached: `{}` ({:.1}%)\n\n",
            s.period,
            format_metric(s.requests, false),
            format_metric(s.prompt_tokens, false),
            format_metric(s.completion_tokens, false),
            format_metric(s.total_tokens, false),
            format_metric(s.cached_tokens, false),
            s.cache_hit_rate(),
        ));
    }
    out
}
