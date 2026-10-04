use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: i64,
    pub timestamp: f64,
    pub date: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    pub token_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub period: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
}

impl UsageSummary {
    pub fn cache_hit_rate(&self) -> f64 {
        if self.prompt_tokens == 0 {
            0.0
        } else {
            (self.cached_tokens as f64 / self.prompt_tokens as f64) * 100.0
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub token_id: i64,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
}

impl TokenUsage {
    pub fn cache_hit_rate(&self) -> f64 {
        if self.prompt_tokens == 0 {
            0.0
        } else {
            (self.cached_tokens as f64 / self.prompt_tokens as f64) * 100.0
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub requests: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageFilter {
    pub model: Option<String>,
    pub token_id: Option<i64>,
}

pub fn format_metric(val: u64, raw: bool) -> String {
    if raw {
        return val.to_string();
    }

    if val < 1_000 {
        val.to_string()
    } else if val < 1_000_000 {
        let k = val as f64 / 1_000.0;
        format!("{:.1}K", k)
    } else if val < 1_000_000_000 {
        let m = val as f64 / 1_000_000.0;
        format!("{:.2}M", m)
    } else {
        let b = val as f64 / 1_000_000_000.0;
        format!("{:.2}B", b)
    }
}
