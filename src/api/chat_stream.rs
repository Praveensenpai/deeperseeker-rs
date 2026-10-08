use crate::api::chat_chunks::{
    build_terminal_chunk, create_openai_chunks, current_timestamp, make_reasoning_chunk,
    make_text_chunk, make_tool_calls_chunk,
};
use crate::api::live_log::{render_input, LogFinish, LogHandle, LogSeed};
use crate::api::state::AppState;
use crate::domain::openai::ChatMessage;
use crate::domain::session::{compute_next_signature, next_parent_id, Session};
use crate::infra::dsml::{find_dsml_block_start, parse_dsml, safe_unambiguous_len, ParsedDsml};
use crate::infra::session_db::save_session;
pub use crate::infra::sse::{
    drain_sse_lines, extract_chunks_from_event, parse_sse_line, ExtractedChunk, SseLineResult,
};
use crate::infra::tokenizer::{count_message_tokens, Tokenizer};
use crate::infra::usage_db::record_usage_attributed;
use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures::{stream, StreamExt};
use std::pin::Pin;
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
    pub reasoning: String,
}

/// Identity and upstream handle for one streaming completion attempt.
pub struct StreamArgs {
    pub req: crate::domain::openai::ChatCompletionRequest,
    pub token_id: i64,
    pub session_id: String,
    pub parent_id: i64,
    pub upstream_resp: reqwest::Response,
    pub client_key_id: Option<i64>,
    pub guard: crate::api::state::CounterGuard,
}

pub async fn handle_streaming_response(
    state: &AppState,
    args: StreamArgs,
) -> Result<Response, (StatusCode, String)> {
    let StreamArgs {
        req,
        token_id,
        session_id,
        parent_id,
        upstream_resp,
        client_key_id,
        guard,
    } = args;
    let chat_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = current_timestamp();
    let db = state.db.clone();
    let state = state.clone();
    let log = state.live_log.begin(LogSeed {
        model: req.model.clone(),
        token_id: Some(token_id),
        session_id: session_id.clone(),
        stream: true,
        input: render_input(&req.messages),
    });

    let prompt_tokens = count_message_tokens(&state.tokenizer, &req.messages);

    // Peek the upstream stream before committing to a 200 SSE response. A
    // context-length rejection must reach the client as a real HTTP error so
    // auto-compacting clients recognize it; once the SSE response is committed
    // only an in-band frame is possible, which those clients do not parse.
    let mut peek_stream = Box::pin(upstream_resp.bytes_stream());
    let (peeked, overflow) = peek_upstream(&mut peek_stream).await;
    if let Some(msg) = overflow {
        log.fail(msg.clone());
        return Err((StatusCode::BAD_REQUEST, msg));
    }

    let sse_stream = async_stream::stream! {
        // Owned by the stream: the in-flight slot is held until this body is
        // fully consumed (or dropped), so the scheduler sees the real load.
        let _guard = guard;
        let mut cur_stream: Pin<Box<dyn futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send>> =
            Box::pin(stream::iter(peeked.into_iter().map(Ok)).chain(peek_stream));
        let mut cur_sess = session_id;
        let mut cur_parent = parent_id;
        let mut cur_tok = token_id;
        let ctx = StreamContext { chat_id: &chat_id, created, model: &req.model };

        for attempt in 0..4 {
            let mut buffer = String::new();
            let mut think_open = false;
            let mut stream_state = StreamState::default();
            let mut raw_lines = Vec::new();

            'outer: while let Some(chunk_res) = cur_stream.next().await {
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
                            for out in process_chunks(chunks, &ctx, &mut stream_state, &log) {
                                yield Ok::<_, std::convert::Infallible>(out);
                            }
                        }
                        SseLineResult::None => {}
                    }
                }
            }

            let parsed = parse_dsml(&stream_state.full_content);
            if let Some(final_chunk) = emit_final_tool_or_text(&ctx, &parsed, &stream_state, &log) {
                yield Ok(final_chunk);
            }

            if !stream_state.full_content.is_empty() || !parsed.tool_calls.is_empty() {
                let comp_tokens = state.tokenizer.count_min_one(&stream_state.full_content);
                let ids = (cur_tok, cur_sess, cur_parent, prompt_tokens, comp_tokens);
                if let Some(term) = finalize_stream_session(&db, &req.messages, &ctx, &parsed, ids, client_key_id, &state.tokenizer).await {
                    yield Ok(format!("data: {term}\n\n"));
                }
                emit_stream_log_success(&log, &req.messages, cur_parent, prompt_tokens, &stream_state, &parsed, &state.tokenizer);
                yield Ok("data: [DONE]\n\n".to_string());
                return;
            }

            // A too-long prompt is identical for every token: never retry it
            // against another token, and surface a recognizable error so
            // auto-compacting clients can recover.
            if let Some(err) = &stream_state.upstream_error {
                if crate::domain::upstream::is_context_overflow_msg(err) {
                    let err_obj = serde_json::json!({
                        "error": {
                            "message": err,
                            "type": "invalid_request_error",
                            "code": "context_length_exceeded"
                        }
                    });
                    log.fail(err.clone());
                    yield Ok(format!("data: {err_obj}\n\n"));
                    yield Ok("data: [DONE]\n\n".to_string());
                    return;
                }
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
                let _ = crate::infra::session_db::delete_sessions_for_chat(&db, cur_tok, &cur_sess).await;

                // An empty stream is almost always a transient upstream hiccup
                // (or a stale session), not a bad credential. Retire the session
                // and try a different token, but leave the token ACTIVE — only a
                // confirmed auth failure may cool a token down. The old code
                // marked every token RATE_LIMITED on a blank response, so four
                // hiccups could sideline four healthy accounts at once.
                let exclude = vec![cur_tok];

                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;

                if let Ok(fresh_prep) = crate::api::chat::create_fresh_session(&state, &req, &exclude).await {
                    if let Ok(comp_args) = crate::api::chat::prepare_completion_args(&state, &req, &fresh_prep).await {
                        if let Ok(new_resp) = state.client.send_completion_request(comp_args).await {
                            cur_sess = fresh_prep.session_id;
                            cur_parent = 0;
                            cur_tok = fresh_prep.token.id;
                            cur_stream = Box::pin(new_resp.bytes_stream());
                            log.reset_output();
                            continue;
                        }
                    }
                }
            }

            tracing::warn!("Empty stream generation for session {cur_sess}; deleting stale session");
            let _ = crate::infra::session_db::delete_sessions_for_chat(&db, cur_tok, &cur_sess).await;
            let err_msg = stream_state
                .upstream_error
                .clone()
                .unwrap_or_else(|| "DeepSeek returned empty completion".to_string());
            let (err_type, err_code) = if crate::domain::upstream::is_context_overflow_msg(&err_msg) {
                ("invalid_request_error", "context_length_exceeded")
            } else if err_msg.contains("Too many requests")
                || err_msg.contains("being generated")
                || err_msg.contains("rate limit")
            {
                ("rate_limit_error", "rate_limit_exceeded")
            } else {
                ("upstream_error", "upstream_error")
            };
            let err_obj = serde_json::json!({
                "error": { "message": err_msg, "type": err_type, "code": err_code }
            });
            log.fail(err_msg);
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

/// Read ahead on the upstream SSE stream far enough to detect a terminal
/// context-overflow rejection, which upstream emits before any content.
///
/// Returns the chunks consumed so the caller can replay them ahead of the
/// untouched remainder, plus the overflow message when one was seen. Content
/// starting (or `[DONE]`) ends the peek immediately so normal streams pay only
/// a tiny, bounded delay.
async fn peek_upstream<S>(stream: &mut S) -> (Vec<Bytes>, Option<String>)
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    let mut read: Vec<Bytes> = Vec::new();
    let mut buffer = String::new();
    let mut think_open = false;

    for _ in 0..16 {
        let Some(chunk) = stream.next().await else {
            break;
        };
        let Ok(bytes) = chunk else {
            break;
        };
        for line in drain_sse_lines(&mut buffer, &bytes) {
            match parse_sse_line(&line, &mut think_open) {
                SseLineResult::Error(_, msg) => {
                    if crate::domain::upstream::is_context_overflow_msg(&msg) {
                        read.push(bytes.clone());
                        return (read, Some(msg));
                    }
                }
                SseLineResult::Chunks(chunks) if !chunks.is_empty() => {
                    read.push(bytes.clone());
                    return (read, None);
                }
                SseLineResult::Done => {
                    read.push(bytes.clone());
                    return (read, None);
                }
                _ => {}
            }
        }
        read.push(bytes);
    }
    (read, None)
}

fn process_chunks(
    chunks: Vec<ExtractedChunk>,
    ctx: &StreamContext,
    state: &mut StreamState,
    log: &LogHandle,
) -> Vec<String> {
    let mut results = Vec::new();
    for chunk in create_openai_chunks(ctx.chat_id, ctx.created, ctx.model, chunks) {
        if let Some(r) = &chunk.choices[0].delta.reasoning_content {
            state.reasoning.push_str(r);
            log.append_reasoning(r);
            let r_chunk = make_reasoning_chunk(ctx.chat_id, ctx.created, ctx.model, r);
            if let Ok(j) = serde_json::to_string(&r_chunk) {
                results.push(format!("data: {j}\n\n"));
            }
        }
        if let Some(text) = &chunk.choices[0].delta.content {
            state.full_content.push_str(text);
            handle_text_chunk(ctx, state, &mut results, log);
        }
    }
    results
}

fn handle_text_chunk(
    ctx: &StreamContext,
    state: &mut StreamState,
    chunks: &mut Vec<String>,
    log: &LogHandle,
) {
    if state.dsml_detected {
        return;
    }
    if let Some(pos) = find_dsml_block_start(&state.full_content) {
        state.dsml_detected = true;
        emit_text_segment(ctx, state, pos, chunks, log);
        return;
    }
    let safe_len = safe_unambiguous_len(&state.full_content);
    emit_text_segment(ctx, state, safe_len, chunks, log);
}

fn emit_text_segment(
    ctx: &StreamContext,
    state: &mut StreamState,
    target_pos: usize,
    chunks: &mut Vec<String>,
    log: &LogHandle,
) {
    if target_pos <= state.streamed_len {
        return;
    }
    let segment = &state.full_content[state.streamed_len..target_pos];
    let chunk = make_text_chunk(ctx.chat_id, ctx.created, ctx.model, segment);
    log.append_content(segment);
    if let Ok(json_str) = serde_json::to_string(&chunk) {
        chunks.push(format!("data: {json_str}\n\n"));
    }
    state.streamed_len = target_pos;
}

fn emit_final_tool_or_text(
    ctx: &StreamContext,
    parsed: &ParsedDsml,
    state: &StreamState,
    log: &LogHandle,
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
            log.append_content(&cleaned);
            let chunk = make_text_chunk(ctx.chat_id, ctx.created, ctx.model, &cleaned);
            serde_json::to_string(&chunk)
                .ok()
                .map(|j| format!("data: {j}\n\n"))
        }
    } else {
        None
    }
}

fn emit_stream_log_success(
    log: &LogHandle,
    messages: &[ChatMessage],
    parent_id: i64,
    prompt_tokens: u32,
    state: &StreamState,
    parsed: &ParsedDsml,
    tokenizer: &Tokenizer,
) {
    let comp_tokens = tokenizer.count_min_one(&state.full_content);
    let cached_tokens = compute_cached_tokens(tokenizer, parent_id, messages, prompt_tokens);
    let finish_reason = if parsed.tool_calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    log.finish(LogFinish {
        prompt_tokens,
        completion_tokens: comp_tokens,
        cached_tokens,
        finish_reason: finish_reason.to_string(),
        tool_calls: parsed.tool_calls.len(),
    });
}

async fn finalize_stream_session(
    db: &tokio_rusqlite::Connection,
    req_messages: &[ChatMessage],
    ctx: &StreamContext<'_>,
    parsed: &ParsedDsml,
    ids: (i64, String, i64, u32, u32),
    client_key_id: Option<i64>,
    tokenizer: &Tokenizer,
) -> Option<String> {
    let (token_id, session_id, parent_id, prompt_tokens, comp_tokens) = ids;
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
        let _ = crate::infra::session_db::delete_sessions_for_chat(db, token_id, &session_id).await;
    }
    let cached_tokens = compute_cached_tokens(tokenizer, parent_id, req_messages, prompt_tokens);
    let _ = record_usage_attributed(
        db,
        ctx.model,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
        Some(token_id),
        client_key_id,
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

pub(crate) fn compute_cached_tokens(
    tokenizer: &Tokenizer,
    parent_id: i64,
    req_messages: &[ChatMessage],
    prompt_tokens: u32,
) -> u32 {
    if parent_id == 0 {
        return 0;
    }
    let last_tokens = req_messages
        .last()
        .map(|m| tokenizer.count(&m.text_content()))
        .unwrap_or(0);
    prompt_tokens.saturating_sub(last_tokens.max(1))
}

pub(crate) async fn save_stream_session(
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
