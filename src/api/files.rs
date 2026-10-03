use crate::api::state::AppState;
use crate::infra::db::{pick_token, record_file};
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn upload_file_openai(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (filename, content_type, bytes) = extract_multipart(&mut multipart).await?;
    let in_flight = state.get_in_flight_snapshot().await;
    let token = pick_token(&state.db, &[], &in_flight, state.config.token_concurrency)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": {"message": "No active tokens available"}})),
            )
        })?;

    let pow_challenge = state
        .client
        .create_pow_challenge(&token.token, "/api/v0/file/upload_file")
        .await
        .map_err(internal_error)?;

    let pow_resp = state
        .pow_solver
        .solve(&pow_challenge, "/api/v0/file/upload_file")
        .map_err(internal_error)?;

    let file_size = bytes.len();
    let file_id = state
        .client
        .upload_file(
            &token.token,
            &pow_resp,
            &filename,
            &content_type,
            bytes,
        )
        .await
        .map_err(internal_error)?;

    record_file(&state.db, &file_id, token.id)
        .await
        .map_err(internal_error)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(Json(json!({
        "id": file_id,
        "object": "file",
        "bytes": file_size,
        "created_at": now,
        "filename": filename,
        "purpose": "vision"
    })))
}

pub async fn upload_file_anthropic(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let res = upload_file_openai(State(state), multipart).await?;
    let Json(val) = res;
    let file_id = val.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let filename = val
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let bytes = val.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);

    Ok(Json(json!({
        "id": file_id,
        "type": "file",
        "filename": filename,
        "size": bytes,
        "created_at": "2026-10-03T12:00:00Z"
    })))
}

async fn extract_multipart(
    multipart: &mut Multipart,
) -> Result<(String, String, Vec<u8>), (StatusCode, Json<serde_json::Value>)> {
    let mut filename = "upload.bin".to_string();
    let mut content_type = "application/octet-stream".to_string();
    let mut file_bytes = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            if let Some(fname) = field.file_name() {
                filename = fname.to_string();
            }
            if let Some(ctype) = field.content_type() {
                content_type = ctype.to_string();
            }
            if let Ok(data) = field.bytes().await {
                file_bytes = data.to_vec();
            }
            break;
        }
    }

    if file_bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "Empty file or missing 'file' field"}})),
        ));
    }

    Ok((filename, content_type, file_bytes))
}

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {"message": format!("{err}")}})),
    )
}
