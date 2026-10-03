use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowChallenge {
    pub algorithm: Option<String>,
    pub challenge: String,
    pub salt: String,
    pub difficulty: u64,
    pub expire_at: i64,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowChallengeWrapper {
    pub code: i64,
    pub data: PowChallengeBizData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowChallengeBizData {
    pub biz_data: PowChallengeInner,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowChallengeInner {
    pub challenge: PowChallenge,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PowSolution {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub answer: u64,
    pub signature: String,
    pub target_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateChatResponse {
    pub code: i64,
    pub data: CreateChatBizData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateChatBizData {
    pub biz_data: CreateChatInner,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateChatInner {
    pub chat_session: ChatSessionObject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatSessionObject {
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_id: String,
    pub token_id: i64,
    pub created_at: f64,
}
