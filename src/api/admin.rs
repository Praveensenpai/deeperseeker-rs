//! Programmatic JSON administration API for the upstream token pool.
//!
//! Authenticated by the master `DEEPSEEKER_API_KEY` (Bearer). Per-client keys
//! are rejected: pool mutation is an operator action. Raw credentials are never
//! returned; every read exposes only the masked form.

use crate::api::state::AppState;
use crate::api::token_admin::verify_upstream;
use crate::domain::token::Token;
use crate::domain::usage::TokenUsage;
use crate::infra::db::{
    add_token as db_add_token, delete_token as db_delete_token, get_token as db_get_token,
    get_tokens, mark_active as db_mark_active, mark_expired as db_mark_expired,
    reset_token_usage as db_reset_token_usage, update_token as db_update_token,
};
use crate::infra::usage_db::get_token_usages;
use axum::{
    extract::{Path, Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

type ApiResult = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

/// Masked token plus its aggregated usage. The plaintext credential is never
/// serialized.
#[derive(Serialize)]
pub struct TokenView {
    pub id: i64,
    pub alias: Option<String>,
    pub masked_token: String,
    pub status: String,
    pub rate_limited_until: Option<f64>,
    pub last_used: Option<f64>,
    pub usage: TokenUsage,
}

#[derive(Deserialize)]
pub struct AddTokenBody {
    pub auth_token: String,
    pub alias: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTokenBody {
    pub auth_token: Option<String>,
    pub alias: Option<String>,
}

#[derive(Deserialize)]
pub struct StatusBody {
    pub status: String,
}

/// Reject any caller that is not presenting the master API key.
pub async fn require_admin_key(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let presented = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match presented {
        Some(token) if token == state.config.api_key => next.run(req).await,
        _ => error(
            StatusCode::UNAUTHORIZED,
            "invalid_request_error",
            "Master API key required for administration",
        )
        .into_response(),
    }
}

pub async fn get_pool(State(state): State<AppState>) -> ApiResult {
    let tokens = get_tokens(&state.db)
        .await
        .map_err(|e| internal(&e.to_string()))?;
    let in_flight: usize = state.in_flight.lock().await.values().sum();

    Ok(Json(json!({
        "status": "ok",
        "pool": {
            "total": tokens.len(),
            "active": tokens.iter().filter(|t| t.is_active()).count(),
            "rate_limited": tokens.iter().filter(|t| t.is_rate_limited()).count(),
            "expired": tokens.iter().filter(|t| t.is_expired()).count(),
            "in_flight": in_flight,
        }
    })))
}

pub async fn list_tokens(State(state): State<AppState>) -> ApiResult {
    let tokens = get_tokens(&state.db)
        .await
        .map_err(|e| internal(&e.to_string()))?;
    let views = build_views(tokens, &state.db).await;
    Ok(Json(json!({ "status": "ok", "tokens": views })))
}

pub async fn get_token(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let token = fetch_token(&state, id).await?;
    let views = build_views(vec![token], &state.db).await;
    match views.into_iter().next() {
        Some(view) => Ok(Json(json!({ "status": "ok", "token": view }))),
        None => Err(not_found(id)),
    }
}

pub async fn add_token(State(state): State<AppState>, Json(body): Json<AddTokenBody>) -> ApiResult {
    let token = clean(&body.auth_token);
    if token.is_empty() {
        return Err(bad_request("auth_token must not be empty"));
    }
    let alias = body
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    db_add_token(&state.db, token, alias)
        .await
        .map_err(|e| bad_request(&format!("Failed to add token: {e}")))?;

    let tokens = get_tokens(&state.db)
        .await
        .map_err(|e| internal(&e.to_string()))?;
    match tokens.into_iter().rfind(|t| t.token == token) {
        Some(tok) => Ok(Json(json!({
            "status": "ok",
            "message": "Token added",
            "token": { "id": tok.id, "alias": tok.alias, "masked_token": tok.masked_token() }
        }))),
        None => Ok(Json(json!({ "status": "ok", "message": "Token added" }))),
    }
}

pub async fn update_token(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateTokenBody>,
) -> ApiResult {
    fetch_token(&state, id).await?;
    let alias = body
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let candidate = body
        .auth_token
        .as_deref()
        .map(clean)
        .filter(|s| !s.is_empty());

    let Some(new_token) = candidate else {
        db_update_token(&state.db, id, None, alias, None)
            .await
            .map_err(|e| internal(&e.to_string()))?;
        return Ok(Json(json!({ "status": "ok", "message": "Alias updated" })));
    };

    if !verify_upstream(&state, new_token).await {
        let _ = db_update_token(&state.db, id, None, alias, Some("EXPIRED")).await;
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": {
                "message": "Token rejected: upstream verification failed",
                "type": "upstream_error"
            }})),
        ));
    }

    db_update_token(&state.db, id, Some(new_token), alias, Some("ACTIVE"))
        .await
        .map_err(|e| internal(&e.to_string()))?;
    Ok(Json(
        json!({ "status": "ok", "message": "Token updated and verified" }),
    ))
}

pub async fn delete_token(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    fetch_token(&state, id).await?;
    db_delete_token(&state.db, id)
        .await
        .map_err(|e| internal(&e.to_string()))?;
    Ok(Json(json!({ "status": "ok", "message": "Token deleted" })))
}

pub async fn verify_token(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let token = fetch_token(&state, id).await?;
    if verify_upstream(&state, &token.token).await {
        let _ = db_mark_active(&state.db, id).await;
        Ok(Json(
            json!({ "status": "ok", "message": "Token verified", "token_status": "ACTIVE" }),
        ))
    } else {
        let _ = db_mark_expired(&state.db, id).await;
        Ok(Json(json!({
            "status": "ok",
            "message": "Verification failed",
            "token_status": "EXPIRED"
        })))
    }
}

pub async fn reset_token(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    fetch_token(&state, id).await?;
    let (usage_rows, session_rows) = db_reset_token_usage(&state.db, id)
        .await
        .map_err(|e| internal(&e.to_string()))?;
    Ok(Json(json!({
        "status": "ok",
        "message": "Usage reset",
        "usage_rows_deleted": usage_rows,
        "session_rows_deleted": session_rows
    })))
}

pub async fn set_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<StatusBody>,
) -> ApiResult {
    fetch_token(&state, id).await?;
    let status = body.status.to_uppercase();
    match status.as_str() {
        "ACTIVE" => {
            db_mark_active(&state.db, id)
                .await
                .map_err(|e| internal(&e.to_string()))?;
        }
        "EXPIRED" | "SUSPENDED" => {
            db_mark_expired(&state.db, id)
                .await
                .map_err(|e| internal(&e.to_string()))?;
        }
        _ => return Err(bad_request("status must be ACTIVE or EXPIRED")),
    }
    Ok(Json(json!({ "status": "ok", "token_status": status })))
}

async fn fetch_token(
    state: &AppState,
    id: i64,
) -> Result<Token, (StatusCode, Json<serde_json::Value>)> {
    match db_get_token(&state.db, id).await {
        Ok(Some(tok)) => Ok(tok),
        Ok(None) => Err(not_found(id)),
        Err(e) => Err(internal(&e.to_string())),
    }
}

async fn build_views(tokens: Vec<Token>, db: &tokio_rusqlite::Connection) -> Vec<TokenView> {
    let usages = get_token_usages(db).await.unwrap_or_default();
    let usage_map: HashMap<i64, TokenUsage> = usages.into_iter().map(|u| (u.token_id, u)).collect();
    tokens
        .into_iter()
        .map(|tok| {
            let masked_token = tok.masked_token();
            let usage = usage_map.get(&tok.id).cloned().unwrap_or(TokenUsage {
                token_id: tok.id,
                ..TokenUsage::default()
            });
            TokenView {
                usage,
                id: tok.id,
                alias: tok.alias,
                masked_token,
                status: tok.status,
                rate_limited_until: tok.rate_limited_until,
                last_used: tok.last_used,
            }
        })
        .collect()
}

/// Trim whitespace and the surrounding quotes users paste from DevTools.
fn clean(raw: &str) -> &str {
    raw.trim().trim_matches('"').trim_matches('\'')
}

fn not_found(id: i64) -> (StatusCode, Json<serde_json::Value>) {
    error(
        StatusCode::NOT_FOUND,
        "not_found",
        &format!("Token {id} not found"),
    )
}

fn bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    error(StatusCode::BAD_REQUEST, "invalid_request_error", message)
}

fn internal(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("Admin API failure: {message}");
    error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
}

fn error(status: StatusCode, kind: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({ "error": { "message": message, "type": kind } })),
    )
}
