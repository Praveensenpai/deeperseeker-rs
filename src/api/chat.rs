use crate::api::chat_stream::{handle_streaming_response, handle_unary_response};
use crate::api::state::AppState;
use crate::domain::openai::ChatCompletionRequest;
use crate::domain::session::compute_signature;
use crate::domain::token::Token;
use crate::domain::upstream::is_auth_failure;
use crate::infra::db::{
    find_session, mark_active, mark_expired, mark_limited, pick_token, touch_token,
};
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
    for _ in 0..5 {
        match execute_completion_attempt(&state, &req, &exclude).await {
            Ok(resp) => return Ok(resp),
            Err(AttemptError::RateLimited(tok_id)) => {
                let _ = mark_limited(&state.db, tok_id, state.config.cookie_cooldown).await;
                exclude.push(tok_id);
            }
            Err(AttemptError::InvalidToken(tok_id, msg)) => {
                handle_token_auth_failure(&state, tok_id, &msg).await;
                exclude.push(tok_id);
            }
            Err(AttemptError::Fatal(status, msg)) => {
                return Err((status, Json(json!({"error": {"message": msg}}))));
            }
        }
    }

    Err((
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "error": {
                "message": "All tokens exhausted, suspended, or rate limited. Please retry in a moment.",
                "type": "rate_limit_error",
                "code": "rate_limit_exceeded"
            }
        })),
    ))
}

#[derive(Debug)]
pub(crate) enum AttemptError {
    RateLimited(i64),
    InvalidToken(i64, String),
    Fatal(StatusCode, String),
}

pub(crate) struct PreparedSession {
    pub token: Token,
    pub session_id: String,
    pub parent_id: i64,
    pub is_first: bool,
}

async fn pace_token_request(state: &AppState, last_used: Option<f64>) {
    if let Some(last) = last_used {
        let elapsed = crate::infra::db::now_timestamp() - last;
        let cfg = &state.config;
        let target = crate::infra::pacing::effective_gap(
            cfg.request_gap,
            cfg.request_gap_jitter,
            cfg.human_pause_chance,
            cfg.human_pause_max,
        );
        crate::infra::pacing::sleep_remainder(elapsed, target).await;
    }
}

async fn handle_token_auth_failure(state: &AppState, tok_id: i64, msg: &str) {
    let Ok(Some(tok)) = crate::infra::db::get_token(&state.db, tok_id).await else {
        return;
    };

    match state
        .client
        .create_pow_challenge(&tok.token, "/api/v0/chat/completion")
        .await
    {
        Ok(_) => {
            tracing::warn!(
                "Token #{} chat error ({msg}), but passed standalone verification; keeping ACTIVE",
                tok_id
            );
        }
        Err(e) if is_auth_failure(&e) => {
            tracing::warn!(
                "Token #{} confirmed expired/invalid by upstream ({e:#}); marking EXPIRED",
                tok_id
            );
            let _ = mark_expired(&state.db, tok_id).await;
        }
        Err(e) => {
            tracing::warn!(
                "Token #{} standalone probe failed non-auth error ({e:#}); applying rate limit",
                tok_id
            );
            let _ = mark_limited(&state.db, tok_id, state.config.cookie_cooldown).await;
        }
    }
}

async fn execute_completion_attempt(
    state: &AppState,
    req: &ChatCompletionRequest,
    exclude: &[i64],
) -> Result<Response, AttemptError> {
    let prep = prepare_session(state, req, exclude).await?;
    let token_id = prep.token.id;
    state.increment_in_flight(token_id).await;

    pace_token_request(state, prep.token.last_used).await;

    let res = run_chat_request(state, req, &prep).await;
    state.decrement_in_flight(token_id).await;

    match res {
        Ok(resp) => {
            let _ = touch_token(&state.db, token_id).await;
            let _ = mark_active(&state.db, token_id).await;
            Ok(resp)
        }
        Err(e) => {
            if !prep.is_first {
                tracing::warn!(
                    "Resumed session {} failed ({:?}); deleting and falling back to fresh session",
                    &prep.session_id[..8.min(prep.session_id.len())],
                    e
                );
                let _ = crate::infra::db::delete_sessions_for_chat(
                    &state.db,
                    token_id,
                    &prep.session_id,
                )
                .await;
                let fresh_prep = create_fresh_session(state, req, exclude).await?;
                let fresh_token_id = fresh_prep.token.id;
                state.increment_in_flight(fresh_token_id).await;
                pace_token_request(state, fresh_prep.token.last_used).await;
                let fresh_res = run_chat_request(state, req, &fresh_prep).await;
                state.decrement_in_flight(fresh_token_id).await;
                if fresh_res.is_ok() {
                    let _ = touch_token(&state.db, fresh_token_id).await;
                    let _ = mark_active(&state.db, fresh_token_id).await;
                }
                return fresh_res;
            }
            Err(e)
        }
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

    let elapsed = crate::infra::db::now_timestamp() - sess.last_used;
    let cfg = &state.config;
    let target = crate::infra::pacing::effective_gap(
        cfg.request_gap,
        cfg.request_gap_jitter,
        cfg.human_pause_chance,
        cfg.human_pause_max,
    );
    crate::infra::pacing::sleep_remainder(elapsed, target).await;

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

pub(crate) async fn create_fresh_session(
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

    let session_id = match state.client.create_chat_session(&tok.token).await {
        Ok(id) => id,
        Err(e) => {
            if is_auth_failure(&e) {
                return Err(AttemptError::InvalidToken(tok.id, e.to_string()));
            }
            return Err(AttemptError::Fatal(StatusCode::BAD_GATEWAY, e.to_string()));
        }
    };

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
        .map_err(|e| {
            if is_auth_failure(&e) {
                AttemptError::InvalidToken(prep.token.id, e.to_string())
            } else {
                AttemptError::RateLimited(prep.token.id)
            }
        })?;

    if req.stream {
        handle_streaming_response(
            state,
            req.clone(),
            prep.token.id,
            prep.session_id.clone(),
            prep.parent_id,
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
        .map_err(|(sc, msg)| {
            if sc == StatusCode::TOO_MANY_REQUESTS {
                AttemptError::RateLimited(prep.token.id)
            } else {
                AttemptError::Fatal(sc, msg)
            }
        })
    }
}

pub(crate) async fn prepare_completion_args(
    state: &AppState,
    req: &ChatCompletionRequest,
    prep: &PreparedSession,
) -> Result<CompletionArgs, AttemptError> {
    let prompt = build_prompt_for_turn(&req.messages, req.tools.as_deref(), prep.is_first);
    let target_path = "/api/v0/chat/completion";

    let challenge = match state
        .client
        .create_pow_challenge(&prep.token.token, target_path)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            if is_auth_failure(&e) {
                return Err(AttemptError::InvalidToken(prep.token.id, e.to_string()));
            }
            return Err(AttemptError::RateLimited(prep.token.id));
        }
    };

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
