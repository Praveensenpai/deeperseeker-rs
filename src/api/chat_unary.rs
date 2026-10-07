//! Non-streaming (unary) chat completion handling.

use crate::api::chat_chunks::{build_chat_response, ChatResponseArgs};
use crate::api::chat_stream::{compute_cached_tokens, save_stream_session};
use crate::api::live_log::{render_input, LogFinish, LogSeed};
use crate::api::state::AppState;
use crate::domain::openai::ChatMessage;
use crate::infra::dsml::parse_dsml;
pub use crate::infra::sse::{drain_sse_lines, parse_sse_line, SseLineResult};
use crate::infra::tokenizer::count_message_tokens;
use crate::infra::usage_db::record_usage_attributed;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;

/// Identity and attribution for one unary completion attempt.
pub struct UnaryContext {
    pub model: String,
    pub token_id: i64,
    pub session_id: String,
    pub parent_id: i64,
    pub client_key_id: Option<i64>,
}

pub async fn handle_unary_response(
    state: &AppState,
    ctx: UnaryContext,
    req_messages: &[ChatMessage],
    upstream_resp: reqwest::Response,
) -> Result<Response, (StatusCode, String)> {
    let UnaryContext {
        model,
        token_id,
        session_id,
        parent_id,
        client_key_id,
    } = ctx;

    let log = state.live_log.begin(LogSeed {
        model: model.clone(),
        token_id: Some(token_id),
        session_id: session_id.clone(),
        stream: false,
        input: render_input(req_messages),
    });

    let (full_content, full_reasoning) = read_unary_body(upstream_resp).await;
    let (raw_content, reasoning) = finalize_content(&full_content, &full_reasoning);
    let parsed = parse_dsml(&raw_content);

    if raw_content.is_empty() && parsed.tool_calls.is_empty() {
        let _ =
            crate::infra::session_db::delete_sessions_for_chat(&state.db, token_id, &session_id)
                .await;
        log.fail("DeepSeek returned empty completion");
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

    let prompt_tokens = count_message_tokens(&state.tokenizer, req_messages);
    let comp_tokens = state.tokenizer.count_min_one(&format!(
        "{full_content}{}",
        reasoning.as_deref().unwrap_or("")
    ));
    let cached_tokens =
        compute_cached_tokens(&state.tokenizer, parent_id, req_messages, prompt_tokens);
    let _ = record_usage_attributed(
        &state.db,
        &model,
        prompt_tokens,
        comp_tokens,
        cached_tokens,
        Some(token_id),
        client_key_id,
    )
    .await;

    let tool_count = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0);
    log.set_output(&content, reasoning.as_deref());
    log.finish(LogFinish {
        prompt_tokens,
        completion_tokens: comp_tokens,
        cached_tokens,
        finish_reason: finish_reason.clone(),
        tool_calls: tool_count,
    });

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
