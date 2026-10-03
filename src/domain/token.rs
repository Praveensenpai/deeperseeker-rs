use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Token {
    pub id: i64,
    pub alias: Option<String>,
    pub token: String,
    pub status: String,
    pub rate_limited_until: Option<f64>,
    pub last_used: Option<f64>,
}

impl Token {
    pub fn is_active(&self) -> bool {
        self.status == "ACTIVE"
    }

    pub fn is_rate_limited(&self) -> bool {
        self.status == "RATE_LIMITED"
    }

    pub fn is_expired_rate_limit(&self, now: f64) -> bool {
        if let Some(until) = self.rate_limited_until {
            now >= until
        } else {
            true
        }
    }

    pub fn masked_token(&self) -> String {
        let len = self.token.chars().count();
        if len <= 16 {
            return "***".to_string();
        }
        let prefix: String = self.token.chars().take(12).collect();
        let suffix: String = self.token.chars().skip(len - 4).collect();
        format!("{prefix}…{suffix}")
    }
}
