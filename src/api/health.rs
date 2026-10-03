use crate::api::state::AppState;
use crate::infra::db::get_tokens;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect},
    Json,
};
use serde_json::json;

pub async fn root() -> impl IntoResponse {
    Redirect::to("/dashboard")
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let tokens = get_tokens(&state.db).await.unwrap_or_default();
    let total = tokens.len();
    let active = tokens.iter().filter(|t| t.is_active()).count();
    let in_flight_map = state.in_flight.lock().await;
    let in_flight_count: usize = in_flight_map.values().sum();

    Json(json!({
        "status": "ok",
        "service": "deeperseeker-rs",
        "version": env!("CARGO_PKG_VERSION"),
        "tokens_total": total,
        "tokens_active": active,
        "in_flight": in_flight_count,
    }))
}
