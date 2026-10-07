use serde::{Deserialize, Serialize};

/// A downstream API key issued to a client, with an optional token budget.
///
/// The plaintext key is never stored; only its SHA-256 hash and a short
/// display prefix are persisted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientKey {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    /// Total-token budget per window. `0` means unlimited.
    pub quota_tokens: u64,
    /// Rolling window length in seconds. `0` means all time.
    pub window_secs: u64,
    pub revoked: bool,
    pub created_at: f64,
    pub last_used: Option<f64>,
}

impl ClientKey {
    pub fn is_active(&self) -> bool {
        !self.revoked
    }

    /// Whether a used-token count has reached this key's budget.
    pub fn is_exhausted(&self, used_tokens: u64) -> bool {
        self.quota_tokens > 0 && used_tokens >= self.quota_tokens
    }

    /// Human-readable window label for the dashboard.
    pub fn window_label(&self) -> String {
        match self.window_secs {
            0 => "all time".to_string(),
            86400 => "day".to_string(),
            604800 => "week".to_string(),
            2592000 => "month".to_string(),
            secs => format!("{}s", secs),
        }
    }
}
