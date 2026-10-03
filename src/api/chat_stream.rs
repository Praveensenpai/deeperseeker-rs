use crate::api::state::AppState;
use crate::domain::openai::{
    ChatChoice, ChatCompletionChunk, ChatCompletionResponse, ChatMessage, ChunkChoice,
    ChunkDelta, ResponseMessage, Usage,
};
use crate::domain::session::{compute_next_signature, next_parent_id, Session};
use crate::infra::db::save_session;
use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub struct ExtractedChunk {
    pub content: Option<String>,
    pub reasoning: Option<String>,
}

pub enum SseLineResult {
    Done,
    Chunks(Vec<ExtractedChunk>),
    None,
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
        let Ok(bytes) = chunk_res else { continue; };
        let lines = drain_sse_lines(&mut buffer, &bytes);
        if process_unary_lines(&lines, &mut think_open, &mut full_content, &mut full_reasoning) {
            break;
        }
    }

    let (content, reasoning) = finalize_content(&full_content, &full_reasoning);
    let next_sig = compute_next_signature(req_messages, &model, &content);
    let next_parent = next_parent_id(parent_id);
    let sess = Session::new(token_id, session_id, next_parent, 0.0);
    let _ = save_session(&state.db, &next_sig, &sess).await;

    let resp = build_chat_response(&model, content, reasoning);
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

fn drain_sse_lines(buffer: &mut String, bytes: &[u8]) -> Vec<String> {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    let mut lines = Vec::new();
    while let Some(pos) = buffer.find('\n') {
        let line = buffer[..pos].trim().to_string();
        *buffer = buffer[pos + 1..].to_string();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn parse_sse_line(line: &str, think_open: &mut bool) -> SseLineResult {
    let maybe_data = line
        .strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"));

    let Some(data) = maybe_data else {
        return SseLineResult::None;
    };

    if data == "[DONE]" {
        return SseLineResult::Done;
    }

    let Ok(json_val) = serde_json::from_str::<serde_json::Value>(data) else {
        return SseLineResult::None;
    };

    let chunks = extract_chunks_from_event(&json_val, think_open);
    SseLineResult::Chunks(chunks)
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

fn build_chat_response(model: &str, content: String, reasoning: Option<String>) -> ChatCompletionResponse {
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
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
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

pub fn extract_chunks_from_event(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Vec<ExtractedChunk> {
    update_think_state(val, think_open);

    if is_status_event(val) {
        return Vec::new();
    }
    if let Some(chunks) = extract_batch_chunks(val, think_open) {
        return chunks;
    }
    if let Some(chunks) = extract_fragment_chunks(val, think_open) {
        return chunks;
    }
    extract_string_delta_chunks(val, *think_open)
}

fn update_think_state(val: &serde_json::Value, think_open: &mut bool) {
    let Some(p) = val.get("p").and_then(|p| p.as_str()) else {
        return;
    };
    if p.contains("/fragments/1") || p.contains("/fragments/2") {
        *think_open = false;
    } else if p.contains("/fragments/0") {
        *think_open = true;
    }
}

fn is_status_event(val: &serde_json::Value) -> bool {
    val.get("p")
        .and_then(|p| p.as_str())
        .map(|p| p.ends_with("/status") || p == "quasi_status")
        .unwrap_or(false)
}

fn extract_batch_chunks(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Option<Vec<ExtractedChunk>> {
    if val.get("o").and_then(|o| o.as_str()) != Some("BATCH") {
        return None;
    }
    let items = val.get("v").and_then(|v| v.as_array())?;
    let mut results = Vec::new();
    for item in items {
        results.extend(extract_chunks_from_event(item, think_open));
    }
    Some(results)
}

fn extract_fragment_chunks(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Option<Vec<ExtractedChunk>> {
    let frags = extract_fragments(val)?;
    let mut results = Vec::new();

    for f in frags {
        let f_type = f.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let content = f.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if f_type == "THINK" {
            *think_open = true;
            if !content.is_empty() {
                results.push(ExtractedChunk {
                    content: None,
                    reasoning: Some(content.to_string()),
                });
            }
        } else if f_type == "RESPONSE" {
            *think_open = false;
            if !content.is_empty() {
                results.push(ExtractedChunk {
                    content: Some(content.to_string()),
                    reasoning: None,
                });
            }
        }
    }
    Some(results)
}

fn extract_string_delta_chunks(val: &serde_json::Value, think_open: bool) -> Vec<ExtractedChunk> {
    let Some(v_str) = val.get("v").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    if v_str.is_empty() {
        return Vec::new();
    }

    if think_open {
        vec![ExtractedChunk {
            content: None,
            reasoning: Some(v_str.to_string()),
        }]
    } else {
        vec![ExtractedChunk {
            content: Some(v_str.to_string()),
            reasoning: None,
        }]
    }
}

fn extract_fragments(val: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    if val.get("p").and_then(|p| p.as_str()) == Some("response/fragments") {
        return val.get("v").and_then(|v| v.as_array());
    }

    val.get("v")
        .and_then(|v| v.get("response"))
        .and_then(|r| r.get("fragments"))
        .and_then(|f| f.as_array())
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
