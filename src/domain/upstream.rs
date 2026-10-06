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
pub struct DeepSeekApiResponse<T> {
    pub code: i64,
    #[serde(default)]
    pub msg: Option<String>,
    pub data: Option<T>,
}

impl<T> DeepSeekApiResponse<T> {
    pub fn extract_data(self) -> anyhow::Result<T> {
        if self.code != 0 {
            let msg = self.msg.unwrap_or_else(|| "Unknown error".to_string());
            if self.code == 40003
                || msg.to_lowercase().contains("authorization failed")
                || msg.to_lowercase().contains("invalid token")
            {
                return Err(anyhow::anyhow!(
                    "Invalid token (code {}): {}",
                    self.code,
                    msg
                ));
            }
            return Err(anyhow::anyhow!(
                "DeepSeek API error (code {}): {}",
                self.code,
                msg
            ));
        }
        self.data
            .ok_or_else(|| anyhow::anyhow!("Missing response data from upstream"))
    }
}

pub fn is_auth_failure(err: &anyhow::Error) -> bool {
    let s = format!("{err:#}").to_lowercase();
    s.contains("invalid token") || s.contains("authorization failed") || s.contains("40003")
}

pub type PowChallengeWrapper = DeepSeekApiResponse<PowChallengeBizData>;

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

pub type CreateChatResponse = DeepSeekApiResponse<CreateChatBizData>;

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
