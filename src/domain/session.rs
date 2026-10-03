use crate::domain::openai::{ChatMessage, ToolCall};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub token_id: i64,
    pub session_id: String,
    pub parent_message_id: i64,
    pub last_used: f64,
}

impl Session {
    pub fn new(token_id: i64, session_id: String, parent_message_id: i64, last_used: f64) -> Self {
        Self {
            token_id,
            session_id,
            parent_message_id,
            last_used,
        }
    }
}

pub fn next_parent_id(current_parent_id: i64) -> i64 {
    current_parent_id + 2
}

#[derive(Serialize)]
struct CanonicalMessage<'a> {
    role: &'a str,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<CanonicalToolCall<'a>>,
}

#[derive(Serialize)]
struct CanonicalToolCall<'a> {
    name: &'a str,
    arguments: &'a str,
}

pub fn compute_signature(messages: &[ChatMessage], model: &str, scope: &str) -> String {
    let mut last_ast_idx = None;
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "assistant" {
            last_ast_idx = Some(i);
            break;
        }
    }

    let history: &[ChatMessage] = match last_ast_idx {
        Some(idx) => &messages[..=idx],
        None => messages,
    };

    let canonical: Vec<CanonicalMessage> = history
        .iter()
        .map(|m| CanonicalMessage {
            role: &m.role,
            content: m.text_content(),
            tool_calls: m
                .tool_calls
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|tc| CanonicalToolCall {
                    name: &tc.function.name,
                    arguments: &tc.function.arguments,
                })
                .collect(),
        })
        .collect();

    let serialized = serde_json::to_string(&canonical).unwrap_or_default();
    let payload = format!("{model}_{scope}_{serialized}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn compute_next_signature(
    messages: &[ChatMessage],
    model: &str,
    assistant_content: &str,
    tool_calls: Option<&[ToolCall]>,
) -> String {
    let mut next_messages = messages.to_vec();
    let mut ast_msg = ChatMessage::assistant(assistant_content);
    ast_msg.tool_calls = tool_calls.map(|tc| tc.to_vec());
    next_messages.push(ast_msg);
    compute_signature(&next_messages, model, "")
}
