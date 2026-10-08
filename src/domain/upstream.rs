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

/// Detect a provider rejection caused by the prompt exceeding the model's
/// context window. Such a request is malformed for the chosen model, not a
/// sign of a bad or rate-limited token, so it must not cool down the pool or
/// be retried against another token. Clients that auto-compact (for example
/// OpenCode) key off this class of error to recover.
pub fn is_context_overflow_msg(msg: &str) -> bool {
    let s = msg.to_lowercase();
    // Deliberately context-specific: a bare "token limit" or "maximum length"
    // can appear in a genuine rate-limit message, which must keep its own
    // retry/cooldown semantics rather than become a client 400.
    s.contains("context length")
        || s.contains("context_length")
        || s.contains("maximum context")
        || s.contains("context window")
        || s.contains("prompt is too long")
        || s.contains("input is too long")
        || s.contains("too many tokens")
}

pub fn is_context_overflow(err: &anyhow::Error) -> bool {
    is_context_overflow_msg(&format!("{err:#}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_overflow_messages_are_detected() {
        assert!(is_context_overflow_msg("maximum context length exceeded"));
        assert!(is_context_overflow_msg(
            "The prompt is too long for this model"
        ));
        assert!(is_context_overflow_msg("input is too long"));
        assert!(is_context_overflow_msg("exceeded the context window"));
        assert!(is_context_overflow_msg("too many tokens in request"));
    }

    #[test]
    fn rate_limit_messages_are_not_misclassified_as_overflow() {
        // These must keep their retry/cooldown semantics.
        assert!(!is_context_overflow_msg("Too many requests"));
        assert!(!is_context_overflow_msg(
            "There is a message being generated"
        ));
        assert!(!is_context_overflow_msg("rate limit exceeded"));
        assert!(!is_context_overflow_msg(
            "DeepSeek returned empty completion"
        ));
    }

    #[test]
    fn anyhow_error_is_classified() {
        let overflow = anyhow::anyhow!("DeepSeek upstream error: maximum context length exceeded");
        assert!(is_context_overflow(&overflow));

        let limited = anyhow::anyhow!("DeepSeek upstream HTTP 429: too many requests");
        assert!(!is_context_overflow(&limited));
        assert!(!is_auth_failure(&limited));
    }
}
