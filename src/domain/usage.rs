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
    pub token_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub period: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub requests: u64,
    pub total_tokens: u64,
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
