use crate::api::state::AppState;
use crate::domain::token::Token;
use crate::domain::usage::{format_metric, TokenUsage};
use crate::infra::db::{add_token as db_add_token, delete_token as db_delete_token, get_tokens};
use axum::{
    extract::{Form, Path, State},
    http::{
        header::{COOKIE, SET_COOKIE},
        HeaderMap, StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tera::Context;

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct AddTokenForm {
    pub auth_token: String,
    pub alias: Option<String>,
}

#[derive(Serialize)]
pub struct DashboardTokenView {
    pub id: i64,
    pub alias: Option<String>,
    pub masked: String,
    pub status: String,
    pub requests: String,
    pub prompt_tokens: String,
    pub completion_tokens: String,
    pub total_tokens: String,
    pub cached_tokens: String,
    pub cache_rate: String,
}

#[derive(Serialize)]
pub struct DashboardSummaryView {
    pub period: String,
    pub requests: String,
    pub prompt_tokens: String,
    pub completion_tokens: String,
    pub total_tokens: String,
    pub cached_tokens: String,
    pub cache_rate: String,
    pub raw_requests: u64,
    pub raw_prompt_tokens: u64,
    pub raw_completion_tokens: u64,
    pub raw_total_tokens: u64,
    pub raw_cached_tokens: u64,
}

pub async fn show_login(State(state): State<AppState>) -> Response {
    let ctx = Context::new();
    let rendered = state.tera.render("login.html", &ctx).unwrap_or_default();
    Html(rendered).into_response()
}

pub async fn submit_login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    if form.username == state.config.admin_user && form.password == state.config.admin_pass {
        let auth_val = auth_hash(&state.config.admin_user, &state.config.session_secret);
        let cookie_val =
            format!("session={auth_val}; Path=/; Max-Age=2592000; HttpOnly; SameSite=Lax");
        let mut resp = Redirect::to("/dashboard").into_response();
        resp.headers_mut().insert(
            SET_COOKIE,
            cookie_val
                .parse()
                .unwrap_or(axum::http::HeaderValue::from_static("")),
        );
        return resp;
    }

    let mut ctx = Context::new();
    ctx.insert("error", "Invalid username or password");
    let rendered = state.tera.render("login.html", &ctx).unwrap_or_default();
    (StatusCode::UNAUTHORIZED, Html(rendered)).into_response()
}

pub async fn logout() -> Response {
    let mut resp = Redirect::to("/login").into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        "session=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT"
            .parse()
            .unwrap_or(axum::http::HeaderValue::from_static("")),
    );
    resp
}

pub async fn show_dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authenticated(&state, &headers) {
        return Redirect::to("/login").into_response();
    }

    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let summaries = crate::infra::usage_db::get_all_summaries(&state.db)
        .await
        .unwrap_or_default();
    let token_usages = crate::infra::usage_db::get_token_usages(&state.db)
        .await
        .unwrap_or_default();
    let active_count = tokens.iter().filter(|t| t.is_active()).count();
    let in_flight_map = state.in_flight.lock().await;
    let in_flight_count: usize = in_flight_map.values().sum();

    let views = build_token_views(tokens, &token_usages);
    let today_sum = summaries.iter().find(|s| s.period == "Today");
    let (cache_hit_rate, today_cached) = match today_sum {
        Some(t) => (
            format!("{:.1}%", t.cache_hit_rate()),
            format_metric(t.cached_tokens, false),
        ),
        None => ("0.0%".to_string(), "0".to_string()),
    };
    let summary_views = map_summary_views(&summaries);

    let mut ctx = Context::new();
    ctx.insert("tokens", &views);
    ctx.insert("summaries", &summary_views);
    ctx.insert("active_count", &active_count);
    ctx.insert("total_tokens_count", &views.len());
    ctx.insert("in_flight", &in_flight_count);
    ctx.insert("cache_hit_rate", &cache_hit_rate);
    ctx.insert("today_cached", &today_cached);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("port", &state.config.port);
    ctx.insert("api_key", &state.config.api_key);
    let rendered = state
        .tera
        .render("dashboard.html", &ctx)
        .unwrap_or_default();
    Html(rendered).into_response()
}

fn map_summary_views(
    summaries: &[crate::domain::usage::UsageSummary],
) -> Vec<DashboardSummaryView> {
    summaries
        .iter()
        .map(|s| DashboardSummaryView {
            period: s.period.clone(),
            requests: format_metric(s.requests, false),
            prompt_tokens: format_metric(s.prompt_tokens, false),
            completion_tokens: format_metric(s.completion_tokens, false),
            total_tokens: format_metric(s.total_tokens, false),
            cached_tokens: format_metric(s.cached_tokens, false),
            cache_rate: format!("{:.1}%", s.cache_hit_rate()),
            raw_requests: s.requests,
            raw_prompt_tokens: s.prompt_tokens,
            raw_completion_tokens: s.completion_tokens,
            raw_total_tokens: s.total_tokens,
            raw_cached_tokens: s.cached_tokens,
        })
        .collect()
}

fn build_token_views(tokens: Vec<Token>, usages: &[TokenUsage]) -> Vec<DashboardTokenView> {
    let usage_map: HashMap<i64, &TokenUsage> = usages.iter().map(|u| (u.token_id, u)).collect();
    tokens
        .into_iter()
        .map(|tok| {
            let usage = usage_map.get(&tok.id).copied();
            let (reqs, p, c, tot, ca, rate) = match usage {
                Some(u) => (
                    format_metric(u.requests, false),
                    format_metric(u.prompt_tokens, false),
                    format_metric(u.completion_tokens, false),
                    format_metric(u.total_tokens, false),
                    format_metric(u.cached_tokens, false),
                    format!("{:.1}%", u.cache_hit_rate()),
                ),
                None => (
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0.0%".to_string(),
                ),
            };
            DashboardTokenView {
                id: tok.id,
                masked: tok.masked_token(),
                alias: tok.alias,
                status: tok.status,
                requests: reqs,
                prompt_tokens: p,
                completion_tokens: c,
                total_tokens: tot,
                cached_tokens: ca,
                cache_rate: rate,
            }
        })
        .collect()
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

    if !token.is_empty() {
        let _ = db_add_token(&state.db, token, alias).await;
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

fn auth_hash(user: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{user}:{secret}").as_bytes());
    hex::encode(hasher.finalize())
}

fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let cookie_hdr = headers
        .get(COOKIE)
        .and_then(|c| c.to_str().ok())
        .unwrap_or("");
    let expected = auth_hash(&state.config.admin_user, &state.config.session_secret);

    for part in cookie_hdr.split(';') {
        let trimmed = part.trim();
        if let Some(val) = trimmed.strip_prefix("session=") {
            if val == expected {
                return true;
            }
        }
    }
    false
}
