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
}

pub async fn handle_streaming_response(
    state: &AppState,
    model: String,
    token_id: i64,
    session_id: String,
    parent_id: i64,
    req_messages: Vec<ChatMessage>,
    upstream_resp: reqwest::Response,
) -> Result<Response, (StatusCode, String)> {
    let chat_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created = current_timestamp();
    let db = state.db.clone();

    let prompt_chars: usize = req_messages.iter().map(|m| m.text_content().len()).sum();
    let prompt_tokens = std::cmp::max(1, (prompt_chars / 4) as u32);
    let byte_stream = upstream_resp.bytes_stream();

    let sse_stream = async_stream::stream! {
        let mut byte_stream = byte_stream;
        let mut buffer = String::new();
        let mut think_open = false;
        let mut stream_state = StreamState::default();
        let ctx = StreamContext { chat_id: &chat_id, created, model: &model };

        'outer: while let Some(chunk_res) = byte_stream.next().await {
            let Ok(bytes) = chunk_res else { continue; };
            for line in drain_sse_lines(&mut buffer, &bytes) {
                match parse_sse_line(&line, &mut think_open) {
                    SseLineResult::Done => break 'outer,
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

        let ids = (token_id, session_id, parent_id, prompt_tokens, stream_state.full_content.len());
        if let Some(term) = finalize_stream_session(&db, &req_messages, &ctx, &parsed, ids).await {
            yield Ok(format!("data: {term}\n\n"));
        }
        yield Ok("data: [DONE]\n\n".to_string());
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
        let tool_chunk = make_tool_calls_chunk(
            ctx.chat_id,
            ctx.created,
            ctx.model,
            parsed.tool_calls.clone(),
        );
        serde_json::to_string(&tool_chunk)
            .ok()
            .map(|j| format!("data: {j}\n\n"))
    } else if state.streamed_len < state.full_content.len() {
        let chunk = make_text_chunk(
            ctx.chat_id,
            ctx.created,
            ctx.model,
            &state.full_content[state.streamed_len..],
        );
        serde_json::to_string(&chunk)
            .ok()
            .map(|j| format!("data: {j}\n\n"))
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
    save_stream_session(
        db,
        req_messages,
        ctx.model,
        parsed,
        token_id,
        session_id,
        parent_id,
    )
    .await;
    let comp_tokens = std::cmp::max(1, (full_len / 4) as u32);
    let _ = record_usage(db, ctx.model, prompt_tokens, comp_tokens, Some(token_id)).await;
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
        finish_reason,
    )
}

async fn save_stream_session(
    db: &tokio_rusqlite::Connection,
    messages: &[ChatMessage],
    model: &str,
    parsed: &ParsedDsml,
    token_id: i64,
    session_id: String,
    parent_id: i64,
) {
    let tools = if parsed.tool_calls.is_empty() {
        None
    } else {
        Some(parsed.tool_calls.as_slice())
    };
    let sig = compute_next_signature(messages, model, &parsed.text_content, tools);
    let next_parent = next_parent_id(parent_id);
    let sess = Session::new(token_id, session_id, next_parent, 0.0);
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

    save_stream_session(
        &state.db,
        req_messages,
        &model,
        &parsed,
        token_id,
        session_id,
        parent_id,
    )
    .await;

    let (content, tool_calls, finish_reason) = if parsed.tool_calls.is_empty() {
        (raw_content, None, "stop".to_string())
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
    let _ = record_usage(
        &state.db,
        &model,
        prompt_tokens,
        comp_tokens,
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
    });
    Ok(Json(resp).into_response())
}

async fn read_unary_body(upstream_resp: reqwest::Response) -> (String, String) {
    let mut byte_stream = upstream_resp.bytes_stream();
    let mut full_content = String::new();
    let mut full_reasoning = String::new();
    let mut buffer = String::new();
    let mut think_open = false;

    while let Some(chunk_res) = byte_stream.next().await {
        let Ok(bytes) = chunk_res else {
            continue;
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
            SseLineResult::Chunks(chunks) => {
                accumulate_chunks(chunks, full_content, full_reasoning);
            }
            SseLineResult::None => {}
        }
    }
    false
}

fn accumulate_chunks(
    chunks: Vec<ExtractedChunk>,
    full_content: &mut String,
    full_reasoning: &mut String,
) {
    for item in chunks {
        if let Some(c) = item.content {
            full_content.push_str(&c);
        }
        if let Some(r) = item.reasoning {
            full_reasoning.push_str(&r);
        }
    }
}

fn finalize_content(full_content: &str, full_reasoning: &str) -> (String, Option<String>) {
    let trimmed_c = full_content.trim();
    let trimmed_r = full_reasoning.trim();

    if trimmed_c.is_empty() && !trimmed_r.is_empty() {
        (trimmed_r.to_string(), None)
    } else {
        let reasoning = if trimmed_r.is_empty() {
            None
        } else {
            Some(trimmed_r.to_string())
        };
        (trimmed_c.to_string(), reasoning)
    }
}
