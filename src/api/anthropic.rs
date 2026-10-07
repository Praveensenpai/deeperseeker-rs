use crate::api::chat::chat_completions;
use crate::api::middleware::ClientAuth;
use crate::api::state::AppState;
use crate::domain::anthropic::{
    AnthropicBlock, AnthropicContent, AnthropicMessage, AnthropicMessageRequest,
    AnthropicMessageResponse, AnthropicUsage,
};
use crate::domain::openai::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ContentPart, MessageContent,
};
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub async fn anthropic_messages(
    State(state): State<AppState>,
    auth: Option<Extension<ClientAuth>>,
    Json(req): Json<AnthropicMessageRequest>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let messages = convert_to_chat_messages(req.system, req.messages);
    let openai_req = ChatCompletionRequest {
        model: req.model.clone(),
        messages,
        stream: req.stream,
        temperature: req.temperature,
        max_tokens: Some(req.max_tokens),
        tools: None,
        reasoning_effort: None,
        thinking: req.thinking,
        search: None,
        web_search: None,
    };

    let resp = chat_completions(State(state), auth, Json(openai_req)).await?;
    if req.stream {
        return Ok(resp);
    }

    parse_anthropic_unary_response(req.model, resp).await
}

fn convert_to_chat_messages(
    system: Option<String>,
    anthropic_msgs: Vec<AnthropicMessage>,
) -> Vec<ChatMessage> {
    let mut messages = Vec::new();

    if let Some(sys) = system {
        if !sys.trim().is_empty() {
            messages.push(ChatMessage::system(sys));
        }
    }

    for m in anthropic_msgs {
        let content = convert_anthropic_content(m.content);
        messages.push(ChatMessage {
            role: m.role,
            content,
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    messages
}

fn convert_anthropic_content(content: AnthropicContent) -> MessageContent {
    match content {
        AnthropicContent::Text(t) => MessageContent::Text(t),
        AnthropicContent::Blocks(blocks) => {
            let parts: Vec<ContentPart> = blocks
                .into_iter()
                .filter_map(convert_anthropic_block)
                .collect();
            if parts.is_empty() {
                MessageContent::None
            } else {
                MessageContent::Parts(parts)
            }
        }
    }
}

fn convert_anthropic_block(b: AnthropicBlock) -> Option<ContentPart> {
    if b.r#type == "text" {
        return b.text.map(ContentPart::text);
    }

    if b.r#type == "image" {
        return b.source.map(|src| {
            ContentPart::image_url(format!("data:{};base64,{}", src.media_type, src.data))
        });
    }

    None
}

async fn parse_anthropic_unary_response(
    model: String,
    resp: Response,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let body_bytes = axum::body::to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": e.to_string()}})),
            )
        })?;

    let openai_res: ChatCompletionResponse = serde_json::from_slice(&body_bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": e.to_string()}})),
        )
    })?;

    let text_content = openai_res
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    let anthropic_res = AnthropicMessageResponse {
        id: openai_res.id,
        r#type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![AnthropicBlock {
            r#type: "text".to_string(),
            text: Some(text_content),
            source: None,
        }],
        model,
        stop_reason: Some("end_turn".to_string()),
        usage: AnthropicUsage {
            input_tokens: openai_res.usage.prompt_tokens,
            output_tokens: openai_res.usage.completion_tokens,
        },
    };

    Ok(Json(anthropic_res).into_response())
}
