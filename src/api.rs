pub mod anthropic;
pub mod chat;
pub mod chat_chunks;
pub mod chat_stream;
pub mod dashboard;
pub mod files;
pub mod health;
pub mod middleware;
pub mod models;
pub mod state;
pub mod usage;

use crate::api::anthropic::anthropic_messages;
use crate::api::chat::chat_completions;
use crate::api::dashboard::{
    add_token, delete_token, edit_token, logout, show_dashboard, show_login, submit_login,
    verify_token,
};
use crate::api::files::{upload_file_anthropic, upload_file_openai};
use crate::api::health::{health, root};
use crate::api::middleware::require_api_key;
use crate::api::models::list_models;
use crate::api::state::AppState;
use crate::api::usage::get_usage_metrics;
use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

pub fn build_router(state: AppState) -> Router {
    let api_routes = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/files", post(upload_file_openai))
        .route("/v1/files/upload", post(upload_file_anthropic))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    let public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/models", get(list_models))
        .route("/v1/models", get(list_models))
        .route("/v1/usage", get(get_usage_metrics))
        .route("/api/usage", get(get_usage_metrics))
        .route("/login", get(show_login).post(submit_login))
        .route("/logout", get(logout))
        .route("/dashboard", get(show_dashboard))
        .route("/tokens/add", post(add_token))
        .route("/tokens/{token_id}/edit", post(edit_token))
        .route("/tokens/{token_id}/verify", post(verify_token))
        .route("/tokens/{token_id}/delete", post(delete_token));

    Router::new()
        .merge(api_routes)
        .merge(public_routes)
        .nest_service(
            "/static",
            ServeDir::new(crate::infra::assets::resolve_asset_dir("static")),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}
