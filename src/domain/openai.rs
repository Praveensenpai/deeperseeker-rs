use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum MessageContent {
    #[default]
    None,
    Text(String),
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn as_text(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ContentPart {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

impl ContentPart {
    pub fn text(t: impl Into<String>) -> Self {
        Self {
            r#type: "text".to_string(),
            text: Some(t.into()),
            ..Default::default()
        }
    }

    pub fn image_url(url: impl Into<String>) -> Self {
        Self {
            r#type: "image_url".to_string(),
            image_url: Some(ImageUrl { url: url.into() }),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileReference {
    pub file_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: MessageContent::Text(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    pub fn text_content(&self) -> String {
        self.content.as_text()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default = "default_model")]
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
}

impl ChatCompletionRequest {
    pub fn is_reasoning_requested(&self) -> bool {
        let model_lower = self.model.to_lowercase();
        if model_lower.contains("reasoner") || model_lower.contains("r1") || model_lower.contains("think") {
            return true;
        }
        if let Some(effort) = &self.reasoning_effort {
            if !effort.is_empty() && effort != "none" {
                return true;
            }
        }
        if let Some(t) = &self.thinking {
            if let Some(type_str) = t.get("type").and_then(|v| v.as_str()) {
                if type_str == "disabled" {
                    return false;
                }
            }
            return true;
        }
        false
    }

    pub fn is_search_requested(&self) -> bool {
        let model_lower = self.model.to_lowercase();
        if model_lower.contains("search") || model_lower.contains("online") {
            return true;
        }
        if self.search == Some(true) || self.web_search == Some(true) {
            return true;
        }
        if let Some(tools) = &self.tools {
            return tools.iter().any(|t| {
                t.get("type").and_then(|v| v.as_str()) == Some("web_search")
                    || t.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        == Some("web_search")
            });
        }
        false
    }
}

fn default_model() -> String {
    "v4.1flash".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Usage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkChoice {
    pub index: usize,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelObject {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelObject>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_reasoning_requested() {
        let chat_req = ChatCompletionRequest {
            model: "deepseek-chat".to_string(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            reasoning_effort: None,
            thinking: None,
            search: None,
            web_search: None,
        };
        assert!(!chat_req.is_reasoning_requested());

        let reasoner_req = ChatCompletionRequest {
            model: "deepseek-reasoner".to_string(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            reasoning_effort: None,
            thinking: None,
            search: None,
            web_search: None,
        };
        assert!(reasoner_req.is_reasoning_requested());

        let r1_req = ChatCompletionRequest {
            model: "deepseek-r1".to_string(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            reasoning_effort: None,
            thinking: None,
            search: None,
            web_search: None,
        };
        assert!(r1_req.is_reasoning_requested());

        let mut effort_req = chat_req.clone();
        effort_req.reasoning_effort = Some("medium".to_string());
        assert!(effort_req.is_reasoning_requested());

        let mut thinking_req = chat_req.clone();
        thinking_req.thinking = Some(serde_json::json!({"type": "enabled"}));
        assert!(thinking_req.is_reasoning_requested());

        let mut disabled_thinking = chat_req.clone();
        disabled_thinking.thinking = Some(serde_json::json!({"type": "disabled"}));
        assert!(!disabled_thinking.is_reasoning_requested());
    }

    #[test]
    fn test_is_search_requested() {
        let mut req = ChatCompletionRequest {
            model: "v4.1flash".to_string(),
            messages: vec![],
            stream: false,
            temperature: None,
            max_tokens: None,
            tools: None,
            reasoning_effort: None,
            thinking: None,
            search: None,
            web_search: None,
        };
        assert!(!req.is_search_requested());

        req.search = Some(true);
        assert!(req.is_search_requested());

        req.search = None;
        req.web_search = Some(true);
        assert!(req.is_search_requested());

        req.web_search = None;
        req.model = "v4.1flash-search".to_string();
        assert!(req.is_search_requested());
    }
}
