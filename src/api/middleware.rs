use crate::api::state::AppState;
use axum::{
    extract::{Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub async fn require_api_key(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(val) if val.starts_with("Bearer ") => &val[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": {
                        "message": "Missing or invalid Authorization header",
                        "type": "invalid_request_error"
                    }
                })),
            )
                .into_response();
        }
    };

    if token != state.config.api_key {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "message": "Incorrect API key provided",
                    "type": "invalid_request_error"
                }
            })),
        )
            .into_response();
    }

    next.run(req).await
}
