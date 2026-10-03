use crate::api::chat_stream::{handle_streaming_response, handle_unary_response};
use crate::api::state::AppState;
use crate::domain::openai::ChatCompletionRequest;
use crate::domain::session::compute_signature;
use crate::domain::token::Token;
use crate::infra::db::{find_session, mark_active, mark_limited, pick_token, touch_token};
use crate::infra::deepseek_client::CompletionArgs;
use crate::infra::media::{resolve_message_media, MediaContext};
use crate::infra::prompt::build_prompt_for_turn;
use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde_json::json;

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    if req.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "messages array cannot be empty"}})),
        ));
    }

    let mut exclude = Vec::new();
    for _ in 0..3 {
        match execute_completion_attempt(&state, &req, &exclude).await {
            Ok(resp) => return Ok(resp),
            Err(AttemptError::RateLimited(tok_id)) => {
                let _ = mark_limited(&state.db, tok_id, state.config.cookie_cooldown).await;
                exclude.push(tok_id);
            }
            Err(AttemptError::Fatal(status, msg)) => {
                return Err((status, Json(json!({"error": {"message": msg}}))));
            }
        }
    }

    Err((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": {"message": "All tokens exhausted or rate limited"}})),
    ))
}

enum AttemptError {
    RateLimited(i64),
    Fatal(StatusCode, String),
}

struct PreparedSession {
    token: Token,
    session_id: String,
    parent_id: i64,
    is_first: bool,
}

async fn execute_completion_attempt(
    state: &AppState,
    req: &ChatCompletionRequest,
    exclude: &[i64],
) -> Result<Response, AttemptError> {
    let prep = prepare_session(state, req, exclude).await?;
    let token_id = prep.token.id;
    state.increment_in_flight(token_id).await;

    let res = run_chat_request(state, req, &prep).await;
    state.decrement_in_flight(token_id).await;

    match res {
        Ok(resp) => {
            let _ = touch_token(&state.db, token_id).await;
            let _ = mark_active(&state.db, token_id).await;
            Ok(resp)
        }
        Err(e) => Err(e),
    }
}

async fn prepare_session(
    state: &AppState,
    req: &ChatCompletionRequest,
    exclude: &[i64],
) -> Result<PreparedSession, AttemptError> {
    if let Some(prep) = try_resume_session(state, req, exclude).await? {
        return Ok(prep);
    }
    create_fresh_session(state, req, exclude).await
}

async fn try_resume_session(
    state: &AppState,
    req: &ChatCompletionRequest,
    exclude: &[i64],
) -> Result<Option<PreparedSession>, AttemptError> {
    if !req.messages.iter().any(|m| m.role == "assistant") {
        return Ok(None);
    }

    let sig = compute_signature(&req.messages, &req.model, "");
    let existing = find_session(&state.db, &sig)
        .await
        .map_err(|e| AttemptError::Fatal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let Some(sess) = existing else {
        return Ok(None);
    };

    let Ok(Some(tok)) = crate::infra::db::get_token(&state.db, sess.token_id).await else {
        return Ok(None);
    };

    if !tok.is_active() || exclude.contains(&tok.id) {
        return Ok(None);
    }

    tracing::info!(
        "Resumed session {} (parent: {}) for sig {}",
        &sess.session_id[..8.min(sess.session_id.len())],
        sess.parent_message_id,
        &sig[..8.min(sig.len())]
    );

    Ok(Some(PreparedSession {
        token: tok,
        session_id: sess.session_id,
        parent_id: sess.parent_message_id,
        is_first: false,
    }))
}

async fn create_fresh_session(
    state: &AppState,
    _req: &ChatCompletionRequest,
    exclude: &[i64],
) -> Result<PreparedSession, AttemptError> {
    let in_flight = state.get_in_flight_snapshot().await;
    let tok = pick_token(
        &state.db,
        exclude,
        &in_flight,
        state.config.token_concurrency,
    )
    .await
    .map_err(|e| AttemptError::Fatal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| {
        AttemptError::Fatal(
            StatusCode::SERVICE_UNAVAILABLE,
            "No active tokens available".to_string(),
        )
    })?;

    let session_id = state
        .client
        .create_chat_session(&tok.token)
        .await
        .map_err(|e| AttemptError::Fatal(StatusCode::BAD_GATEWAY, e.to_string()))?;

    tracing::info!(
        "Created fresh upstream session {} for token {}",
        &session_id[..8.min(session_id.len())],
        tok.id
    );

    Ok(PreparedSession {
        token: tok,
        session_id,
        parent_id: 0,
        is_first: true,
    })
}

async fn run_chat_request(
    state: &AppState,
    req: &ChatCompletionRequest,
    prep: &PreparedSession,
) -> Result<Response, AttemptError> {
    let args = prepare_completion_args(state, req, prep).await?;
    let resp = state
        .client
        .send_completion_request(args)
        .await
        .map_err(|_| AttemptError::RateLimited(prep.token.id))?;

    if req.stream {
        handle_streaming_response(
            state,
            req.model.clone(),
            prep.token.id,
            prep.session_id.clone(),
            prep.parent_id,
            req.messages.clone(),
            resp,
        )
        .await
        .map_err(|(sc, msg)| AttemptError::Fatal(sc, msg))
    } else {
        handle_unary_response(
            state,
            req.model.clone(),
            prep.token.id,
            prep.session_id.clone(),
            prep.parent_id,
            &req.messages,
            resp,
        )
        .await
        .map_err(|(sc, msg)| AttemptError::Fatal(sc, msg))
    }
}

async fn prepare_completion_args(
    state: &AppState,
    req: &ChatCompletionRequest,
    prep: &PreparedSession,
) -> Result<CompletionArgs, AttemptError> {
    let prompt = build_prompt_for_turn(&req.messages, req.tools.as_deref(), prep.is_first);
    let target_path = "/api/v0/chat/completion";

    let challenge = state
        .client
        .create_pow_challenge(&prep.token.token, target_path)
        .await
        .map_err(|_| AttemptError::RateLimited(prep.token.id))?;

    let pow_resp = state
        .pow_solver
        .solve(&challenge, target_path)
        .map_err(|e| AttemptError::Fatal(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let media_ctx = MediaContext {
        db: &state.db,
        client: &state.client,
        solver: &state.pow_solver,
        token_id: prep.token.id,
        token: &prep.token.token,
    };
    let ref_file_ids = resolve_message_media(&media_ctx, &req.messages)
        .await
        .map_err(|e| AttemptError::Fatal(StatusCode::BAD_REQUEST, e.to_string()))?;

    let parent_msg_id = if prep.parent_id == 0 {
        None
    } else {
        Some(prep.parent_id)
    };

    let thinking_enabled = req.is_reasoning_requested();
    let search_enabled = req.is_search_requested();

    Ok(CompletionArgs {
        token: prep.token.token.clone(),
        session_id: prep.session_id.clone(),
        parent_message_id: parent_msg_id,
        prompt,
        pow_response: pow_resp,
        ref_file_ids,
        thinking_enabled,
        search_enabled,
    })
}
