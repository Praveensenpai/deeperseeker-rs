use crate::api::chat_chunks::{
    build_chat_response, build_terminal_chunk, create_openai_chunks, current_timestamp,
    make_reasoning_chunk, make_text_chunk, make_tool_calls_chunk, ChatResponseArgs,
};
use crate::api::state::AppState;
use crate::domain::openai::ChatMessage;
use crate::domain::session::{compute_next_signature, next_parent_id, Session};
use crate::infra::db::save_session;
use crate::infra::dsml::{find_dsml_block_start, parse_dsml, safe_unambiguous_len, ParsedDsml};
pub use crate::infra::sse::{
    drain_sse_lines, extract_chunks_from_event, parse_sse_line, ExtractedChunk, SseLineResult,
};
use crate::infra::usage_db::record_usage;
use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use uuid::Uuid;

struct StreamContext<'a> {
    pub chat_id: &'a str,
    pub created: u64,
    pub model: &'a str,
}

#[derive(Default)]
struct StreamState {
    pub full_content: String,
    pub streamed_len: usize,
    pub dsml_detected: bool,
    pub upstream_error: Option<String>,
}

pub async fn handle_streaming_response(
    state: &AppState,
    req: crate::domain::openai::ChatCompletionRequest,
    token_id: i64,
    session_id: String,
    parent_id: i64,
    upstream_resp: reqwest::Response,
) -> Result<Response, (StatusCode, String)> {
    let chat_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = current_timestamp();
    let db = state.db.clone();
    let state = state.clone();

    let prompt_chars: usize = req.messages.iter().map(|m| m.text_content().len()).sum();
    let prompt_tokens = std::cmp::max(1, (prompt_chars / 4) as u32);

    let sse_stream = async_stream::stream! {
        let mut cur_resp = upstream_resp;
        let mut cur_sess = session_id;
        let mut cur_parent = parent_id;
        let mut cur_tok = token_id;
        let ctx = StreamContext { chat_id: &chat_id, created, model: &req.model };

        for attempt in 0..4 {
            let mut byte_stream = cur_resp.bytes_stream();
            let mut buffer = String::new();
            let mut think_open = false;
            let mut stream_state = StreamState::default();
            let mut raw_lines = Vec::new();

            'outer: while let Some(chunk_res) = byte_stream.next().await {
                let Ok(bytes) = chunk_res else {
                    tracing::warn!("Upstream stream dropped mid-generation");
                    break 'outer;
                };
                for line in drain_sse_lines(&mut buffer, &bytes) {
                    if raw_lines.len() < 5 {
                        raw_lines.push(line.clone());
                    }
                    match parse_sse_line(&line, &mut think_open) {
                        SseLineResult::Done => break 'outer,
                        SseLineResult::Error(code, msg) => {
                            tracing::warn!("Upstream error in SSE stream (code {code}): {msg}");
                            stream_state.upstream_error = Some(msg);
                            break 'outer;
                        }
                        SseLineResult::Chunks(chunks) => {
                            for out in process_chunks(chunks, &ctx, &mut stream_state) {
                                yield Ok::<_, std::convert::Infallible>(out);
                            }
                        }
                        SseLineResult::None => {}
                    }
                }
            }

            let parsed = parse_dsml(&stream_state.full_content);
            if let Some(final_chunk) = emit_final_tool_or_text(&ctx, &parsed, &stream_state) {
                yield Ok(final_chunk);
            }

            if !stream_state.full_content.is_empty() || !parsed.tool_calls.is_empty() {
                let ids = (cur_tok, cur_sess, cur_parent, prompt_tokens, stream_state.full_content.len());
                if let Some(term) = finalize_stream_session(&db, &req.messages, &ctx, &parsed, ids).await {
                    yield Ok(format!("data: {term}\n\n"));
                }
                yield Ok("data: [DONE]\n\n".to_string());
                return;
            }

            tracing::warn!(
                "Stream produced 0 tokens (attempt {attempt}). Raw lines: {:?}",
                raw_lines
            );

            if attempt < 3 {
                let factor = match attempt {
                    0 => 1.0,
                    1 => 1.5,
                    _ => 2.0,
                };
                let base = state.config.request_gap * factor;
                let gap = crate::infra::pacing::effective_gap(
                    base,
                    state.config.request_gap_jitter,
                    state.config.human_pause_chance,
                    state.config.human_pause_max,
                );
                let wait_ms = (gap * 1000.0) as u64;
                tracing::warn!("Retrying with fresh session after {wait_ms}ms...");
                let _ = crate::infra::db::delete_sessions_for_chat(&db, cur_tok, &cur_sess).await;

                let _ = crate::infra::db::mark_limited(&db, cur_tok, state.config.cookie_cooldown).await;
                let exclude = vec![cur_tok];

                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;

                if let Ok(fresh_prep) = crate::api::chat::create_fresh_session(&state, &req, &exclude).await {
                    if let Ok(comp_args) = crate::api::chat::prepare_completion_args(&state, &req, &fresh_prep).await {
                        if let Ok(new_resp) = state.client.send_completion_request(comp_args).await {
                            cur_sess = fresh_prep.session_id;
                            cur_parent = 0;
                            cur_tok = fresh_prep.token.id;
                            cur_resp = new_resp;
                            continue;
                        }
                    }
                }
            }

            tracing::warn!("Empty stream generation for session {cur_sess}; deleting stale session");
            let _ = crate::infra::db::delete_sessions_for_chat(&db, cur_tok, &cur_sess).await;
            let err_msg = stream_state
                .upstream_error
                .unwrap_or_else(|| "DeepSeek returned empty completion".to_string());
            let is_rate_limit = err_msg.contains("Too many requests")
                || err_msg.contains("being generated")
                || err_msg.contains("rate limit");
            let (err_type, err_code) = if is_rate_limit {
                ("rate_limit_error", "rate_limit_exceeded")
            } else {
                ("upstream_error", "upstream_error")
            };
            let err_obj = serde_json::json!({
                "error": { "message": err_msg, "type": err_type, "code": err_code }
            });
            yield Ok(format!("data: {err_obj}\n\n"));
            yield Ok("data: [DONE]\n\n".to_string());
            return;
        }
    };

    Ok(Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(sse_stream))
        .unwrap_or_default())
}

fn process_chunks(
    chunks: Vec<ExtractedChunk>,
    ctx: &StreamContext,
    state: &mut StreamState,
) -> Vec<String> {
    let mut results = Vec::new();
    for chunk in create_openai_chunks(ctx.chat_id, ctx.created, ctx.model, chunks) {
        if let Some(r) = &chunk.choices[0].delta.reasoning_content {
            let r_chunk = make_reasoning_chunk(ctx.chat_id, ctx.created, ctx.model, r);
            if let Ok(j) = serde_json::to_string(&r_chunk) {
                results.push(format!("data: {j}\n\n"));
            }
        }
        if let Some(text) = &chunk.choices[0].delta.content {
            state.full_content.push_str(text);
            handle_text_chunk(ctx, state, &mut results);
        }
    }
    results
}

fn handle_text_chunk(ctx: &StreamContext, state: &mut StreamState, chunks: &mut Vec<String>) {
    if state.dsml_detected {
        return;
    }
    if let Some(pos) = find_dsml_block_start(&state.full_content) {
        state.dsml_detected = true;
        emit_text_segment(ctx, state, pos, chunks);
        return;
    }
    let safe_len = safe_unambiguous_len(&state.full_content);
    emit_text_segment(ctx, state, safe_len, chunks);
}

fn emit_text_segment(
    ctx: &StreamContext,
    state: &mut StreamState,
    target_pos: usize,
    chunks: &mut Vec<String>,
) {
    if target_pos <= state.streamed_len {
        return;
    }
    let chunk = make_text_chunk(
        ctx.chat_id,
        ctx.created,
        ctx.model,
        &state.full_content[state.streamed_len..target_pos],
    );
    if let Ok(json_str) = serde_json::to_string(&chunk) {
        chunks.push(format!("data: {json_str}\n\n"));
    }
    state.streamed_len = target_pos;
}

fn emit_final_tool_or_text(
    ctx: &StreamContext,
    parsed: &ParsedDsml,
    state: &StreamState,
) -> Option<String> {
    if !parsed.tool_calls.is_empty() {
        let chunk = make_tool_calls_chunk(
            ctx.chat_id,
            ctx.created,
            ctx.model,
            parsed.tool_calls.clone(),
        );
        serde_json::to_string(&chunk)
            .ok()
            .map(|j| format!("data: {j}\n\n"))
    } else if state.streamed_len < state.full_content.len() {
        let remaining = &state.full_content[state.streamed_len..];
        let cleaned = crate::infra::dsml::clean_history_text(remaining);
        if cleaned.is_empty() {
            None
        } else {
            let chunk = make_text_chunk(ctx.chat_id, ctx.created, ctx.model, &cleaned);
            serde_json::to_string(&chunk)
                .ok()
                .map(|j| format!("data: {j}\n\n"))
        }
    } else {
        None
    }
}

async fn finalize_stream_session(
    db: &tokio_rusqlite::Connection,
    req_messages: &[ChatMessage],
    ctx: &StreamContext<'_>,
    parsed: &ParsedDsml,
    ids: (i64, String, i64, u32, usize),
) -> Option<String> {
    let (token_id, session_id, parent_id, prompt_tokens, full_len) = ids;
    if parent_id < 40 {
        save_stream_session(
            db,
            req_messages,
            ctx.model,
            parsed,
            (token_id, session_id, parent_id),
        )
        .await;
    } else {
        tracing::info!("Rolling over session at parent {parent_id}");
        let _ = crate::infra::db::delete_sessions_for_chat(db, token_id, &session_id).await;
    }
    let comp_tokens = std::cmp::max(1, (full_len / 4) as u32);
    let cached_tokens = compute_cached_tokens(parent_id, req_messages, prompt_tokens);
    let _ = record_usage(
        db,
        ctx.model,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
        Some(token_id),
    )
    .await;
    let finish_reason = if parsed.tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    build_terminal_chunk(
        ctx.chat_id,
        ctx.created,
        ctx.model,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
        finish_reason,
    )
}

fn compute_cached_tokens(parent_id: i64, req_messages: &[ChatMessage], prompt_tokens: u32) -> u32 {
    if parent_id == 0 {
        return 0;
    }
    let last_chars = req_messages
        .last()
        .map(|m| m.text_content().len())
        .unwrap_or(0);
    prompt_tokens.saturating_sub((last_chars / 4).max(1) as u32)
}

async fn save_stream_session(
    db: &tokio_rusqlite::Connection,
    messages: &[ChatMessage],
    model: &str,
    parsed: &ParsedDsml,
    ids: (i64, String, i64),
) {
    let (token_id, session_id, parent_id) = ids;
    let tools = (!parsed.tool_calls.is_empty()).then_some(parsed.tool_calls.as_slice());
    let sig = compute_next_signature(messages, model, &parsed.text_content, tools);
    let next_parent = next_parent_id(parent_id);
    let now = crate::infra::db::now_timestamp();
    let sess = Session::new(token_id, session_id.clone(), next_parent, now);
    tracing::info!(
        "Saved session {} (next_parent: {next_parent}) for sig {}",
        &session_id[..8.min(session_id.len())],
        &sig[..8.min(sig.len())]
    );
    let _ = save_session(db, &sig, &sess).await;
}

pub async fn handle_unary_response(
    state: &AppState,
    model: String,
    token_id: i64,
    session_id: String,
    parent_id: i64,
    req_messages: &[ChatMessage],
    upstream_resp: reqwest::Response,
) -> Result<Response, (StatusCode, String)> {
    let (full_content, full_reasoning) = read_unary_body(upstream_resp).await;
    let (raw_content, reasoning) = finalize_content(&full_content, &full_reasoning);
    let parsed = parse_dsml(&raw_content);

    if raw_content.is_empty() && parsed.tool_calls.is_empty() {
        let _ = crate::infra::db::delete_sessions_for_chat(&state.db, token_id, &session_id).await;
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "DeepSeek returned empty completion".to_string(),
        ));
    }

    save_stream_session(
        &state.db,
        req_messages,
        &model,
        &parsed,
        (token_id, session_id, parent_id),
    )
    .await;

    let (content, tool_calls, finish_reason) = if parsed.tool_calls.is_empty() {
        (parsed.text_content, None, "stop".to_string())
    } else {
        (
            parsed.text_content,
            Some(parsed.tool_calls),
            "tool_calls".to_string(),
        )
    };

    let prompt_chars: usize = req_messages.iter().map(|m| m.text_content().len()).sum();
    let prompt_tokens = std::cmp::max(1, (prompt_chars / 4) as u32);
    let comp_chars = full_content.len() + reasoning.as_ref().map(|r| r.len()).unwrap_or(0);
    let comp_tokens = std::cmp::max(1, (comp_chars / 4) as u32);
    let cached_tokens = compute_cached_tokens(parent_id, req_messages, prompt_tokens);
    let _ = record_usage(
        &state.db,
        &model,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
        Some(token_id),
    )
    .await;

    let resp = build_chat_response(ChatResponseArgs {
        model: &model,
        content,
        reasoning,
        tool_calls,
        finish_reason: &finish_reason,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
    });
    Ok(Json(resp).into_response())
}

async fn read_unary_body(upstream_resp: reqwest::Response) -> (String, String) {
    let mut byte_stream = upstream_resp.bytes_stream();
    let (mut full_content, mut full_reasoning, mut buffer) =
        (String::new(), String::new(), String::new());
    let mut think_open = false;

    while let Some(chunk_res) = byte_stream.next().await {
        let Ok(bytes) = chunk_res else {
            tracing::warn!("Upstream stream dropped mid-generation (unary)");
            break;
        };
        let lines = drain_sse_lines(&mut buffer, &bytes);
        if process_unary_lines(
            &lines,
            &mut think_open,
            &mut full_content,
            &mut full_reasoning,
        ) {
            break;
        }
    }
    (full_content, full_reasoning)
}

fn process_unary_lines(
    lines: &[String],
    think_open: &mut bool,
    full_content: &mut String,
    full_reasoning: &mut String,
) -> bool {
    for line in lines {
        match parse_sse_line(line, think_open) {
            SseLineResult::Done => return true,
            SseLineResult::Error(code, msg) => {
                tracing::warn!("Upstream error in unary SSE (code {code}): {msg}");
                return true;
            }
            SseLineResult::Chunks(chunks) => {
                for item in chunks {
                    if let Some(c) = item.content {
                        full_content.push_str(&c);
                    }
                    if let Some(r) = item.reasoning {
                        full_reasoning.push_str(&r);
                    }
                }
            }
            SseLineResult::None => {}
        }
    }
    false
}

fn finalize_content(full_content: &str, full_reasoning: &str) -> (String, Option<String>) {
    let (tc, tr) = (full_content.trim(), full_reasoning.trim());
    if tc.is_empty() && !tr.is_empty() {
        (tr.to_string(), None)
    } else {
        (tc.to_string(), (!tr.is_empty()).then(|| tr.to_string()))
    }
}
