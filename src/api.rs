pub mod admin;
pub mod anthropic;
pub mod chat;
pub mod chat_chunks;
pub mod chat_stream;
pub mod chat_support;
pub mod chat_unary;
pub mod client_key_admin;
pub mod dashboard;
pub mod files;
pub mod health;
pub mod live_log;
pub mod logs;
pub mod metrics;
pub mod middleware;
pub mod models;
pub mod state;
pub mod token_admin;
pub mod usage;

use crate::api::admin::{
    add_token as admin_add_token, delete_token as admin_delete_token, get_pool,
    get_token as admin_get_token, list_tokens as admin_list_tokens, require_admin_key,
    reset_token as admin_reset_token, set_status as admin_set_status,
    update_token as admin_update_token, verify_token as admin_verify_token,
};
use crate::api::anthropic::anthropic_messages;
use crate::api::chat::chat_completions;
use crate::api::client_key_admin::{add_client_key, delete_client_key, revoke_client_key};
use crate::api::dashboard::{logout, show_dashboard, show_login, submit_login};
use crate::api::files::{upload_file_anthropic, upload_file_openai};
use crate::api::health::{health, root};
use crate::api::logs::{get_logs, stream_logs};
use crate::api::metrics::get_metrics;
use crate::api::middleware::require_api_key;
use crate::api::models::list_models;
use crate::api::state::AppState;
use crate::api::token_admin::{add_token, delete_token, edit_token, verify_token};
use crate::api::usage::get_usage_metrics;
use axum::{
    extract::DefaultBodyLimit,
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
        // Axum's 2 MB default body limit cannot carry the long contexts this
        // proxy advertises (hundreds of thousands of tokens). 64 MB is a
        // generous ceiling that still bounds memory per request.
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .route_layer(from_fn_with_state(state.clone(), require_api_key));

    let admin_routes = Router::new()
        .route("/api/admin/pool", get(get_pool))
        .route(
            "/api/admin/tokens",
            get(admin_list_tokens).post(admin_add_token),
        )
        .route(
            "/api/admin/tokens/{id}",
            get(admin_get_token)
                .patch(admin_update_token)
                .delete(admin_delete_token),
        )
        .route("/api/admin/tokens/{id}/verify", post(admin_verify_token))
        .route("/api/admin/tokens/{id}/reset", post(admin_reset_token))
        .route("/api/admin/tokens/{id}/status", post(admin_set_status))
        .route_layer(from_fn_with_state(state.clone(), require_admin_key));

    let public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/metrics", get(get_metrics))
        .route("/models", get(list_models))
        .route("/v1/models", get(list_models))
        .route("/v1/usage", get(get_usage_metrics))
        .route("/api/usage", get(get_usage_metrics))
        .route("/login", get(show_login).post(submit_login))
        .route("/logout", get(logout))
        .route("/dashboard", get(show_dashboard))
        .route("/api/logs", get(get_logs))
        .route("/api/logs/stream", get(stream_logs))
        .route("/tokens/add", post(add_token))
        .route("/tokens/{token_id}/edit", post(edit_token))
        .route("/tokens/{token_id}/verify", post(verify_token))
        .route("/tokens/{token_id}/delete", post(delete_token))
        .route("/client-keys/add", post(add_client_key))
        .route("/client-keys/{id}/revoke", post(revoke_client_key))
        .route("/client-keys/{id}/delete", post(delete_client_key));

    Router::new()
        .merge(api_routes)
        .merge(admin_routes)
        .merge(public_routes)
        .nest_service(
            "/static",
            ServeDir::new(crate::infra::assets::resolve_asset_dir("static")),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}
