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

pub fn compute_signature(
    messages: &[crate::domain::openai::ChatMessage],
    model: &str,
    scope: &str,
) -> String {
    let mut last_ast_idx = None;
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "assistant" {
            last_ast_idx = Some(i);
            break;
        }
    }

    let history: &[crate::domain::openai::ChatMessage] = match last_ast_idx {
        Some(idx) => &messages[..=idx],
        None => messages,
    };

    let serialized = serde_json::to_string(history).unwrap_or_default();
    let payload = format!("{model}_{scope}_{serialized}");
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn compute_next_signature(
    messages: &[crate::domain::openai::ChatMessage],
    model: &str,
    assistant_content: &str,
) -> String {
    let mut next_messages = messages.to_vec();
    next_messages.push(crate::domain::openai::ChatMessage {
        role: "assistant".to_string(),
        content: crate::domain::openai::MessageContent::Text(assistant_content.to_string()),
        name: None,
    });
    compute_signature(&next_messages, model, "")
}
