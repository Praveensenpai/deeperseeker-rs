//! Dashboard handlers for managing upstream token pool credentials.

use crate::api::dashboard::{is_authenticated, redirect_msg};
use crate::api::state::AppState;
use crate::infra::db::{
    add_token as db_add_token, delete_expired_tokens as db_delete_expired_tokens,
    delete_token as db_delete_token, get_token as db_get_token, mark_active as db_mark_active,
    mark_expired as db_mark_expired, update_token as db_update_token,
};
use axum::{
    extract::{Form, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AddTokenForm {
    pub auth_token: String,
    pub alias: Option<String>,
}

#[derive(Deserialize)]
pub struct EditTokenForm {
    pub auth_token: Option<String>,
    pub alias: Option<String>,
}

pub async fn add_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AddTokenForm>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let token = form.auth_token.trim().trim_matches('"').trim_matches('\'');
    let alias = form
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if !token.is_empty() && db_add_token(&state.db, token, alias).await.is_ok() {
        if let Ok(tokens) = crate::infra::db::get_tokens(&state.db).await {
            let total_count = tokens.len();
            let active_count = tokens.iter().filter(|t| t.is_active()).count();
            if let Some(tok) = tokens.iter().find(|t| t.token == token) {
                state.telegram.notify(
                    crate::domain::telegram::TelegramNotification::NewTokenAdded {
                        id: tok.id,
                        alias: tok.alias.clone(),
                        masked: tok.masked_token(),
                        active_count,
                        total_count,
                    },
                );
            }
        }
    }

    Redirect::to("/dashboard").into_response()
}

pub async fn delete_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token_id): Path<i64>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let _ = db_delete_token(&state.db, token_id).await;
    Redirect::to("/dashboard").into_response()
}

pub async fn clear_expired_tokens(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    match db_delete_expired_tokens(&state.db).await {
        Ok(0) => redirect_msg("No expired tokens to clear", true),
        Ok(count) => redirect_msg(&format!("Cleared {count} expired token(s)"), true),
        Err(err) => {
            tracing::error!("Failed to clear expired tokens: {err:#}");
            redirect_msg("Failed to clear expired tokens", false)
        }
    }
}

pub async fn edit_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token_id): Path<i64>,
    Form(form): Form<EditTokenForm>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let alias = form
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let candidate = form
        .auth_token
        .as_deref()
        .map(str::trim)
        .map(|s| s.trim_matches('"').trim_matches('\''))
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Alias-only change: no verification needed.
    let Some(new_token) = candidate else {
        let _ = db_update_token(&state.db, token_id, None, alias.as_deref(), None).await;
        return redirect_msg("Alias updated", true);
    };

    if !verify_upstream(&state, &new_token).await {
        // Reject the token value: keep existing credentials, mark EXPIRED.
        let _ = db_update_token(&state.db, token_id, None, alias.as_deref(), Some("EXPIRED")).await;
        return redirect_msg("Token rejected: upstream verification failed", false);
    }

    if db_update_token(
        &state.db,
        token_id,
        Some(&new_token),
        alias.as_deref(),
        Some("ACTIVE"),
    )
    .await
    .is_err()
    {
        return redirect_msg("Failed to save token", false);
    }
    redirect_msg("Token updated and verified", true)
}

pub async fn verify_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token_id): Path<i64>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let Ok(Some(tok)) = db_get_token(&state.db, token_id).await else {
        return redirect_msg("Token not found", false);
    };

    if verify_upstream(&state, &tok.token).await {
        let _ = db_mark_active(&state.db, token_id).await;
        redirect_msg("Token verified: ACTIVE", true)
    } else {
        let _ = db_mark_expired(&state.db, token_id).await;
        let active_count = crate::infra::db::get_tokens(&state.db)
            .await
            .map(|list| list.iter().filter(|t| t.is_active()).count())
            .unwrap_or(0);
        state.telegram.notify(
            crate::domain::telegram::TelegramNotification::TokenExpired {
                id: token_id,
                alias: tok.alias.clone(),
                reason: "Upstream verification failed".to_string(),
                active_count,
            },
        );
        redirect_msg("Token verification failed: marked EXPIRED", false)
    }
}

pub(crate) async fn verify_upstream(state: &AppState, token: &str) -> bool {
    match state
        .client
        .create_pow_challenge(token, "/api/v0/chat/completion")
        .await
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("Token verification failed: {e:#}");
            false
        }
    }
}
