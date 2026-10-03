use crate::api::state::AppState;
use crate::infra::usage_db::{get_all_summaries, get_daily_breakdown, get_model_breakdown};
use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;

pub async fn get_usage_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let summaries = get_all_summaries(&state.db).await.unwrap_or_default();
    let daily = get_daily_breakdown(&state.db, 30).await.unwrap_or_default();
    let models = get_model_breakdown(&state.db).await.unwrap_or_default();

    Json(json!({
        "status": "ok",
        "summaries": summaries,
        "daily": daily,
        "models": models,
    }))
}
