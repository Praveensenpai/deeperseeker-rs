use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelegramSettings {
    pub bot_token: String,
    pub chat_id: String,
    pub enabled: bool,
    pub updated_at: f64,
}

impl TelegramSettings {
    pub fn is_configured(&self) -> bool {
        self.enabled && !self.bot_token.trim().is_empty() && !self.chat_id.trim().is_empty()
    }

    pub fn masked_token(&self) -> String {
        let trimmed = self.bot_token.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if let Some((bot_id, secret)) = trimmed.split_once(':') {
            if secret.len() > 6 {
                let end = &secret[secret.len().saturating_sub(4)..];
                return format!("{bot_id}:••••{end}");
            }
        }
        if trimmed.len() > 8 {
            format!("••••{}", &trimmed[trimmed.len().saturating_sub(6)..])
        } else {
            "••••••••".to_string()
        }
    }
}

#[derive(Clone, Debug)]
pub enum TelegramNotification {
    NewTokenAdded {
        id: i64,
        alias: Option<String>,
        masked: String,
        active_count: usize,
        total_count: usize,
    },
    TokenExpired {
        id: i64,
        alias: Option<String>,
        reason: String,
        active_count: usize,
    },
    NoValidTokens {
        endpoint: String,
        model: String,
        total_tokens: usize,
    },
    Test {
        timestamp: f64,
    },
}

impl TelegramNotification {
    pub fn format_markdown(&self) -> String {
        match self {
            Self::NewTokenAdded {
                id,
                alias,
                masked,
                active_count,
                total_count,
            } => {
                let name = alias.as_deref().unwrap_or("unnamed");
                format!(
                    "🆕 *DeeperSeeker: New Token Added*\n\n\
                     • *Token ID:* `#{id}`\n\
                     • *Alias:* `{name}`\n\
                     • *Key:* `{masked}`\n\
                     • *Pool Health:* `{active_count} active / {total_count} total`"
                )
            }
            Self::TokenExpired {
                id,
                alias,
                reason,
                active_count,
            } => {
                let name = alias.as_deref().unwrap_or("unnamed");
                format!(
                    "⚠️ *DeeperSeeker: Token Expired*\n\n\
                     • *Token ID:* `#{id}` ({name})\n\
                     • *Reason:* `{reason}`\n\
                     • *Active Remaining:* `{active_count}`\n\n\
                     _Please update or rotate this token in the dashboard._"
                )
            }
            Self::NoValidTokens {
                endpoint,
                model,
                total_tokens,
            } => {
                format!(
                    "🚨 *DeeperSeeker Alert: No Valid Tokens Available*\n\n\
                     • *Endpoint:* `{endpoint}`\n\
                     • *Requested Model:* `{model}`\n\
                     • *Pool Size:* `{total_tokens} tokens (0 active/ready)`\n\n\
                     _Incoming requests are failing with HTTP 503/429. Please add or verify upstream tokens immediately!_"
                )
            }
            Self::Test { .. } => {
                "🔔 *DeeperSeeker: Telegram Bot Connected*\n\n\
                 Your Telegram bot integration is active and verified.\n\
                 You will receive alerts here for pool changes and can send `/status`, `/tokens`, or `/usage` at any time."
                    .to_string()
            }
        }
    }
}
