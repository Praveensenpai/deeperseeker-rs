use axum::{
    response::{IntoResponse, Redirect},
    Json,
};
use serde_json::json;

pub async fn root() -> impl IntoResponse {
    Redirect::to("/dashboard")
}

pub async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok", "service": "deeperseeker-rs"}))
}
