//! Dashboard handlers for creating, revoking, and deleting client API keys.

use crate::api::dashboard::{encode_component, is_authenticated, redirect_msg};
use crate::api::state::AppState;
use crate::infra::client_keys::{
    add_client_key as db_add_client_key, delete_client_key as db_delete_client_key,
    set_revoked as db_set_client_key_revoked,
};
use axum::{
    extract::{Form, Path, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AddClientKeyForm {
    pub name: String,
    pub quota_tokens: Option<String>,
    pub window: Option<String>,
}

pub async fn add_client_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AddClientKeyForm>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let name = form.name.trim();
    if name.is_empty() {
        return redirect_msg("Client key name is required", false);
    }

    let quota_tokens = parse_quota(form.quota_tokens.as_deref());
    let window_secs = parse_window(form.window.as_deref());

    match db_add_client_key(&state.db, name, quota_tokens, window_secs).await {
        Ok((_, plaintext)) => Redirect::to(&format!(
            "/dashboard?ok=1&msg={}&new_key={}",
            encode_component("Client key created"),
            encode_component(&plaintext)
        ))
        .into_response(),
        Err(e) => {
            tracing::error!("Failed creating client key: {e:#}");
            redirect_msg("Failed to create client key", false)
        }
    }
}

pub async fn revoke_client_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }
    let _ = db_set_client_key_revoked(&state.db, id, true).await;
    redirect_msg("Client key revoked", true)
}

pub async fn delete_client_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }
    let _ = db_delete_client_key(&state.db, id).await;
    redirect_msg("Client key deleted", true)
}

fn parse_quota(raw: Option<&str>) -> u64 {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

fn parse_window(raw: Option<&str>) -> u64 {
    match raw.map(str::trim) {
        Some("day") => 86_400,
        Some("week") => 604_800,
        Some("month") => 2_592_000,
        _ => 0,
    }
}
