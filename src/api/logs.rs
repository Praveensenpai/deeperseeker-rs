use crate::api::dashboard::is_authenticated;
use crate::api::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        HeaderMap, StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};
use tokio::sync::broadcast::error::RecvError;

/// Snapshot of the buffered request log as JSON.
pub async fn get_logs(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authenticated(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(state.live_log.snapshot()).into_response()
}

/// Server-sent event tail of the live request log.
pub async fn stream_logs(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !is_authenticated(&state, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let snapshot = state.live_log.snapshot();
    let mut receiver = state.live_log.subscribe();

    let stream = async_stream::stream! {
        if let Ok(json) = serde_json::to_string(&snapshot) {
            yield Ok::<_, std::convert::Infallible>(format!("event: snapshot\ndata: {json}\n\n"));
        }
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        yield Ok(format!("data: {json}\n\n"));
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };

    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_default()
}
