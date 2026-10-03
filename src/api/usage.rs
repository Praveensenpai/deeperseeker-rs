use crate::api::state::AppState;
use crate::domain::usage::UsageFilter;
use crate::infra::usage_db::{
    get_filtered_daily_breakdown, get_filtered_model_breakdown, get_filtered_summaries,
};
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, Default)]
pub struct UsageQuery {
    pub model: Option<String>,
    pub token_id: Option<i64>,
}

pub async fn get_usage_metrics(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> impl IntoResponse {
    let filter = UsageFilter {
        model: query.model,
        token_id: query.token_id,
    };
    let summaries = get_filtered_summaries(&state.db, &filter)
        .await
        .unwrap_or_default();
    let daily = get_filtered_daily_breakdown(&state.db, 30, &filter)
        .await
        .unwrap_or_default();
    let models = get_filtered_model_breakdown(&state.db, &filter)
        .await
        .unwrap_or_default();

    Json(json!({
        "status": "ok",
        "filter": {
            "model": filter.model,
            "token_id": filter.token_id,
        },
        "summaries": summaries,
        "daily": daily,
        "models": models,
    }))
}
