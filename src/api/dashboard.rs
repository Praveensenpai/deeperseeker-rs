use crate::api::state::AppState;
use crate::domain::client_key::ClientKey;
use crate::domain::token::Token;
use crate::domain::usage::{format_metric, TokenUsage};
use crate::infra::client_keys::{list_client_keys, window_usage_tokens};
use crate::infra::db::{get_tokens, now_timestamp};
use axum::{
    extract::{Form, Query, State},
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
pub struct MsgQuery {
    pub msg: Option<String>,
    pub ok: Option<String>,
    #[serde(default)]
    pub new_key: Option<String>,
}

#[derive(Serialize)]
pub struct DashboardClientKeyView {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub quota: String,
    pub window: String,
    pub used: String,
    pub used_pct: u64,
    pub revoked: bool,
    pub last_used: String,
}

#[derive(Serialize)]
pub struct DashboardTokenView {
    pub id: i64,
    pub alias: Option<String>,
    pub masked: String,
    pub status: String,
    pub last_used_ago: String,
    pub last_used_title: String,
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

pub async fn show_dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<MsgQuery>,
) -> Response {
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
    let client_keys = list_client_keys(&state.db).await.unwrap_or_default();
    let client_key_views = build_client_key_views(&client_keys, &state.db).await;
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
    ctx.insert("client_keys", &client_key_views);
    ctx.insert("summaries", &summary_views);
    ctx.insert("active_count", &active_count);
    ctx.insert("total_tokens_count", &views.len());
    ctx.insert("in_flight", &in_flight_count);
    ctx.insert("cache_hit_rate", &cache_hit_rate);
    ctx.insert("today_cached", &today_cached);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("port", &state.config.port);
    ctx.insert("api_key", &state.config.api_key);
    ctx.insert("new_key", &query.new_key);
    ctx.insert("flash_msg", &query.msg);
    ctx.insert("flash_ok", &(query.ok.as_deref() == Some("1")));
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

fn format_timestamp_title(secs: i64) -> String {
    let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(secs) else {
        return "Unknown".to_string();
    };
    let fmt =
        time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    dt.format(&fmt).unwrap_or_else(|_| "Unknown".to_string())
}

fn format_relative_time(ts: Option<f64>, now: f64) -> (String, String) {
    let Some(t) = ts.filter(|&v| v > 0.0) else {
        return ("Never".to_string(), "Never used".to_string());
    };

    let elapsed = (now - t).max(0.0);
    let ago = if elapsed < 60.0 {
        format!("{}s ago", elapsed as u64)
    } else if elapsed < 3600.0 {
        format!("{}m ago", (elapsed / 60.0).floor() as u64)
    } else if elapsed < 86400.0 {
        format!("{}h ago", (elapsed / 3600.0).floor() as u64)
    } else {
        format!("{}d ago", (elapsed / 86400.0).floor() as u64)
    };

    (ago, format_timestamp_title(t as i64))
}

fn build_token_views(tokens: Vec<Token>, usages: &[TokenUsage]) -> Vec<DashboardTokenView> {
    let now = now_timestamp();
    let usage_map: HashMap<i64, &TokenUsage> = usages.iter().map(|u| (u.token_id, u)).collect();
    tokens
        .into_iter()
        .map(|tok| {
            let (last_used_ago, last_used_title) = format_relative_time(tok.last_used, now);
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
                last_used_ago,
                last_used_title,
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

async fn build_client_key_views(
    keys: &[ClientKey],
    db: &tokio_rusqlite::Connection,
) -> Vec<DashboardClientKeyView> {
    let now = now_timestamp();
    let mut views = Vec::with_capacity(keys.len());
    for key in keys {
        let used = window_usage_tokens(db, key.id, key.window_secs)
            .await
            .unwrap_or(0);
        let (quota, used_pct) = if key.quota_tokens == 0 {
            ("Unlimited".to_string(), 0)
        } else {
            let pct = used.saturating_mul(100) / key.quota_tokens.max(1);
            (format_metric(key.quota_tokens, false), pct.min(100))
        };
        let (last_used, _) = format_relative_time(key.last_used, now);
        views.push(DashboardClientKeyView {
            id: key.id,
            name: key.name.clone(),
            key_prefix: format!("{}…", key.key_prefix),
            quota,
            window: key.window_label(),
            used: format_metric(used, false),
            used_pct,
            revoked: key.revoked,
            last_used,
        });
    }
    views
}

pub(crate) fn encode_component(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

pub(crate) fn redirect_msg(msg: &str, ok: bool) -> Response {
    let encoded = encode_component(msg);
    let flag = if ok { "1" } else { "0" };
    Redirect::to(&format!("/dashboard?msg={encoded}&ok={flag}")).into_response()
}

fn auth_hash(user: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{user}:{secret}").as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
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
