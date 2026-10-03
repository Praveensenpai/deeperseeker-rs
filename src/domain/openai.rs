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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentPart {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<FileReference>,
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
