use crate::api::state::AppState;
use crate::domain::openai::{
    ChatChoice, ChatCompletionChunk, ChatCompletionResponse, ChatMessage, ChunkChoice, ChunkDelta,
    ResponseMessage, Usage,
};
use crate::domain::session::{compute_next_signature, next_parent_id, Session};
use crate::infra::db::save_session;
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
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

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
        let mut full_content = String::new();

        'outer: while let Some(chunk_res) = byte_stream.next().await {
            let Ok(bytes) = chunk_res else { continue; };
            for line in drain_sse_lines(&mut buffer, &bytes) {
                match parse_sse_line(&line, &mut think_open) {
                    SseLineResult::Done => break 'outer,
                    SseLineResult::Chunks(chunks) => {
                        for chunk in create_openai_chunks(&chat_id, created, &model, chunks) {
                            if let Some(text) = &chunk.choices[0].delta.content {
                                full_content.push_str(text);
                            }
                            if let Ok(json_str) = serde_json::to_string(&chunk) {
                                yield Ok::<_, std::convert::Infallible>(format!("data: {json_str}\n\n"));
                            }
                        }
                    }
                    SseLineResult::None => {}
                }
            }
        }

        let next_sig = compute_next_signature(&req_messages, &model, &full_content);
        let next_parent = next_parent_id(parent_id);
        let sess = Session::new(token_id, session_id, next_parent, 0.0);
        let _ = save_session(&db, &next_sig, &sess).await;

        let comp_tokens = std::cmp::max(1, (full_content.len() / 4) as u32);
        let _ = record_usage(&db, &model, prompt_tokens, comp_tokens, Some(token_id)).await;

        yield Ok("data: [DONE]\n\n".to_string());
    };

    Ok(Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(sse_stream))
        .unwrap_or_default())
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

    let (content, reasoning) = finalize_content(&full_content, &full_reasoning);
    let next_sig = compute_next_signature(req_messages, &model, &content);
    let next_parent = next_parent_id(parent_id);
    let sess = Session::new(token_id, session_id, next_parent, 0.0);
    let _ = save_session(&state.db, &next_sig, &sess).await;

    let prompt_chars: usize = req_messages.iter().map(|m| m.text_content().len()).sum();
    let prompt_tokens = std::cmp::max(1, (prompt_chars / 4) as u32);
    let comp_chars = content.len() + reasoning.as_ref().map(|r| r.len()).unwrap_or(0);
    let comp_tokens = std::cmp::max(1, (comp_chars / 4) as u32);

    let _ = record_usage(
        &state.db,
        &model,
        prompt_tokens,
        comp_tokens,
        Some(token_id),
    )
    .await;

    let resp = build_chat_response(&model, content, reasoning, prompt_tokens, comp_tokens);
    Ok(Json(resp).into_response())
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

fn build_chat_response(
    model: &str,
    content: String,
    reasoning: Option<String>,
    prompt_tokens: u32,
    completion_tokens: u32,
) -> ChatCompletionResponse {
    let total_tokens = prompt_tokens + completion_tokens;
    ChatCompletionResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: current_timestamp(),
        model: model.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".to_string(),
                content,
                reasoning_content: reasoning,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        },
    }
}

fn create_openai_chunks(
    chat_id: &str,
    created: u64,
    model: &str,
    chunks: Vec<ExtractedChunk>,
) -> Vec<ChatCompletionChunk> {
    let mut openai_chunks = Vec::new();
    for item in chunks {
        openai_chunks.push(ChatCompletionChunk {
            id: chat_id.to_string(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: item.content,
                    reasoning_content: item.reasoning,
                },
                finish_reason: None,
            }],
            usage: None,
        });
    }
    openai_chunks
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
