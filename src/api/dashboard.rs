use crate::api::state::AppState;
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
}

pub async fn show_login(State(state): State<AppState>) -> Response {
    let ctx = Context::new();
    let rendered = state.tera.render("login.html", &ctx).unwrap_or_default();
    Html(rendered).into_response()
}

pub async fn submit_login(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    if form.username == state.config.admin_user && form.password == state.config.admin_pass {
        let auth_val = auth_hash(&state.config.admin_user, &state.config.session_secret);
        let cookie_val = format!("session={auth_val}; Path=/; HttpOnly; SameSite=Lax");
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
    let active_count = tokens.iter().filter(|t| t.is_active()).count();
    let in_flight_map = state.in_flight.lock().await;
    let in_flight_count: usize = in_flight_map.values().sum();

    let views: Vec<DashboardTokenView> = tokens
        .into_iter()
        .map(|tok| DashboardTokenView {
            id: tok.id,
            masked: tok.masked_token(),
            alias: tok.alias,
            status: tok.status,
        })
        .collect();

    let mut ctx = Context::new();
    ctx.insert("tokens", &views);
    ctx.insert("summaries", &summaries);
    ctx.insert("active_count", &active_count);
    ctx.insert("total_tokens_count", &views.len());
    ctx.insert("in_flight", &in_flight_count);
    ctx.insert("version", env!("CARGO_PKG_VERSION"));
    ctx.insert("port", &state.config.port);
    ctx.insert("api_key", &state.config.api_key);
    let rendered = state
        .tera
        .render("dashboard.html", &ctx)
        .unwrap_or_default();
    Html(rendered).into_response()
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
