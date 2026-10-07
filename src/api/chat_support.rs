//! Token health and request pacing helpers shared by the chat handlers.

use crate::api::state::AppState;
use crate::domain::upstream::is_auth_failure;
use crate::infra::db::{mark_expired, mark_limited};

/// Sleep just long enough to keep the gap between requests on a token
/// human-like, based on when it was last used.
pub(crate) async fn pace_token_request(state: &AppState, last_used: Option<f64>) {
    if let Some(last) = last_used {
        let elapsed = crate::infra::db::now_timestamp() - last;
        let cfg = &state.config;
        let target = crate::infra::pacing::effective_gap(
            cfg.request_gap,
            cfg.request_gap_jitter,
            cfg.human_pause_chance,
            cfg.human_pause_max,
        );
        crate::infra::pacing::sleep_remainder(elapsed, target).await;
    }
}

/// Decide what to do after a token fails an upstream chat call: probe it
/// standalone so a transient chat error does not wrongly retire a live token.
pub(crate) async fn handle_token_auth_failure(state: &AppState, tok_id: i64, msg: &str) {
    let Ok(Some(tok)) = crate::infra::db::get_token(&state.db, tok_id).await else {
        return;
    };

    match state
        .client
        .create_pow_challenge(&tok.token, "/api/v0/chat/completion")
        .await
    {
        Ok(_) => {
            tracing::warn!(
                "Token #{} chat error ({msg}), but passed standalone verification; keeping ACTIVE",
                tok_id
            );
        }
        Err(e) if is_auth_failure(&e) => {
            tracing::warn!(
                "Token #{} confirmed expired/invalid by upstream ({e:#}); marking EXPIRED",
                tok_id
            );
            let _ = mark_expired(&state.db, tok_id).await;
        }
        Err(e) => {
            tracing::warn!(
                "Token #{} standalone probe failed non-auth error ({e:#}); applying rate limit",
                tok_id
            );
            let _ = mark_limited(&state.db, tok_id, state.config.cookie_cooldown).await;
        }
    }
}
