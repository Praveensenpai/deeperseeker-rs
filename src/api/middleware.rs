use crate::api::state::AppState;
use crate::infra::client_keys::{find_active_by_plaintext, touch_client_key, window_usage_tokens};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::AUTHORIZATION, header::CONTENT_TYPE, header::RETRY_AFTER, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::json;
use tokio::sync::OwnedSemaphorePermit;

/// Resolved caller identity, injected into request extensions by
/// [`require_api_key`]. `client_key_id == None` means the master API key,
/// which is unlimited.
#[derive(Clone, Debug)]
pub struct ClientAuth {
    pub client_key_id: Option<i64>,
}

pub async fn require_api_key(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(val) if val.starts_with("Bearer ") => val[7..].to_string(),
        _ => return unauthorized("Missing or invalid Authorization header"),
    };

    // The master key keeps full, unlimited access (backward compatible).
    if token == state.config.api_key {
        req.extensions_mut().insert(ClientAuth {
            client_key_id: None,
        });
        return next.run(req).await;
    }

    let key = match find_active_by_plaintext(&state.db, &token).await {
        Ok(Some(key)) => key,
        Ok(None) => return unauthorized("Incorrect API key provided"),
        Err(e) => {
            tracing::error!("Client key lookup failed: {e:#}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": "Client key lookup failed"}})),
            )
                .into_response();
        }
    };

    // Quota-limited keys are admitted one request at a time: the previous
    // request's usage is recorded before the next is checked, so concurrent
    // requests can no longer overshoot the rolling window. The permit is held
    // until the response body drains, covering streaming completions too.
    // Unlimited keys (quota == 0) skip the gate and keep full concurrency.
    let permit = if key.quota_tokens > 0 {
        state.acquire_client_gate(key.id).await
    } else {
        None
    };

    let used = window_usage_tokens(&state.db, key.id, key.window_secs)
        .await
        .unwrap_or(0);

    if key.is_exhausted(used) {
        let retry_after = if key.window_secs > 0 {
            key.window_secs
        } else {
            60
        };
        // `permit` drops here, releasing the gate for the next request.
        return quota_exceeded(&key.name, used, retry_after);
    }

    let _ = touch_client_key(&state.db, key.id).await;
    req.extensions_mut().insert(ClientAuth {
        client_key_id: Some(key.id),
    });
    let response = next.run(req).await;

    match permit {
        Some(permit) => hold_permit_until_body_done(response, permit),
        None => response,
    }
}

/// Wrap `response` so `permit` is released only once the body has been fully
/// consumed (or dropped). Capturing the permit in the body stream extends the
/// admission gate over streaming responses, not just header generation.
fn hold_permit_until_body_done(response: Response, permit: OwnedSemaphorePermit) -> Response {
    let (parts, body) = response.into_parts();
    let stream = body.into_data_stream().map(move |item| {
        let _hold = &permit;
        item
    });
    Response::from_parts(parts, Body::from_stream(stream))
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": {
                "message": message,
                "type": "invalid_request_error"
            }
        })),
    )
        .into_response()
}

fn quota_exceeded(name: &str, used: u64, retry_after: u64) -> Response {
    let body = json!({
        "error": {
            "message": format!("Token quota exceeded for API key '{name}'"),
            "type": "rate_limit_error",
            "code": "quota_exceeded",
            "used_tokens": used,
            "retry_after": retry_after
        }
    });
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(RETRY_AFTER, retry_after.to_string())
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap_or_default()
}
