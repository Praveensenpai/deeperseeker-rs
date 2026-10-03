use crate::domain::openai::{
    ChatChoice, ChatCompletionChunk, ChatCompletionResponse, ChunkChoice, ChunkDelta,
    ResponseMessage, ToolCall, Usage,
};
use crate::infra::sse::ExtractedChunk;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub struct ChatResponseArgs<'a> {
    pub model: &'a str,
    pub content: String,
    pub reasoning: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: &'a str,
    pub prompt_tokens: u32,
    pub comp_tokens: u32,
    pub cached_tokens: u32,
}

pub fn build_chat_response(args: ChatResponseArgs) -> ChatCompletionResponse {
    let total_tokens = args.prompt_tokens + args.comp_tokens;
    let details = (args.cached_tokens > 0).then_some(crate::domain::openai::PromptTokensDetails {
        cached_tokens: args.cached_tokens,
    });
    ChatCompletionResponse {
        id: format!("chatcmpl-{}", Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: current_timestamp(),
        model: args.model.to_string(),
        choices: vec![ChatChoice {
            index: 0,
            message: ResponseMessage {
                role: "assistant".to_string(),
                content: args.content,
                reasoning_content: args.reasoning,
                tool_calls: args.tool_calls,
            },
            finish_reason: Some(args.finish_reason.to_string()),
        }],
        usage: Usage {
            prompt_tokens: args.prompt_tokens,
            completion_tokens: args.comp_tokens,
            total_tokens,
            prompt_tokens_details: details,
        },
    }
}

pub fn create_openai_chunks(
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
                    tool_calls: None,
                },
                finish_reason: None,
            }],
            usage: None,
        });
    }
    openai_chunks
}

pub fn make_text_chunk(
    chat_id: &str,
    created: u64,
    model: &str,
    text: &str,
) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: chat_id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: Some(text.to_string()),
                reasoning_content: None,
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

pub fn make_reasoning_chunk(
    chat_id: &str,
    created: u64,
    model: &str,
    reasoning: &str,
) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: chat_id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
                reasoning_content: Some(reasoning.to_string()),
                tool_calls: None,
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

pub fn make_tool_calls_chunk(
    chat_id: &str,
    created: u64,
    model: &str,
    tool_calls: Vec<ToolCall>,
) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: chat_id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
                reasoning_content: None,
                tool_calls: Some(tool_calls),
            },
            finish_reason: None,
        }],
        usage: None,
    }
}

pub fn build_terminal_chunk(
    chat_id: &str,
    created: u64,
    model: &str,
    prompt_tokens: u32,
    comp_tokens: u32,
    cached_tokens: u32,
    finish_reason: &str,
) -> Option<String> {
    let details =
        (cached_tokens > 0).then_some(crate::domain::openai::PromptTokensDetails { cached_tokens });
    let terminal_chunk = ChatCompletionChunk {
        id: chat_id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta::default(),
            finish_reason: Some(finish_reason.to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens,
            completion_tokens: comp_tokens,
            total_tokens: prompt_tokens + comp_tokens,
            prompt_tokens_details: details,
        }),
    };
    serde_json::to_string(&terminal_chunk).ok()
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
