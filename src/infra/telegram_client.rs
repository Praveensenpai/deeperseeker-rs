use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TelegramClient {
    client: Client,
}

#[derive(Serialize)]
struct SendMessageBody<'a> {
    chat_id: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    disable_web_page_preview: bool,
}

#[derive(Deserialize)]
pub struct TelegramApiResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub description: Option<String>,
    pub parameters: Option<TelegramResponseParameters>,
}

#[derive(Deserialize)]
pub struct TelegramResponseParameters {
    pub retry_after: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramIncomingMessage>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramIncomingMessage {
    pub message_id: i64,
    pub chat: TelegramChat,
    pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
}

impl Default for TelegramClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TelegramClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(35))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn send_message(&self, bot_token: &str, chat_id: &str, text: &str) -> Result<()> {
        let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
        let body = SendMessageBody {
            chat_id,
            text,
            parse_mode: Some("Markdown"),
            disable_web_page_preview: true,
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed connecting to Telegram API")?;

        let status = resp.status();
        let api_res: TelegramApiResponse<serde_json::Value> = resp
            .json()
            .await
            .context("Failed parsing Telegram API response JSON")?;

        if api_res.ok {
            return Ok(());
        }

        // Fallback: If Markdown parsing failed upstream, retry once with plain text
        if let Some(desc) = &api_res.description {
            if desc.to_lowercase().contains("can't parse entities") {
                let plain_body = SendMessageBody {
                    chat_id,
                    text,
                    parse_mode: None,
                    disable_web_page_preview: true,
                };
                let retry_resp = self.client.post(&url).json(&plain_body).send().await?;
                if retry_resp.status().is_success() {
                    return Ok(());
                }
            }
        }

        if let Some(params) = api_res.parameters {
            if let Some(wait) = params.retry_after {
                bail!("Telegram rate limited (retry after {wait}s)");
            }
        }

        let desc = api_res.description.unwrap_or_else(|| status.to_string());
        bail!("Telegram API error: {desc}")
    }

    pub async fn get_updates(
        &self,
        bot_token: &str,
        offset: i64,
        timeout_secs: u64,
    ) -> Result<Vec<TelegramUpdate>> {
        let url = format!("https://api.telegram.org/bot{bot_token}/getUpdates");
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "offset": offset,
                "timeout": timeout_secs,
                "allowed_updates": ["message"]
            }))
            .send()
            .await
            .context("Failed calling Telegram getUpdates")?;

        let api_res: TelegramApiResponse<Vec<TelegramUpdate>> = resp
            .json()
            .await
            .context("Failed parsing getUpdates response")?;

        if !api_res.ok {
            let desc = api_res
                .description
                .unwrap_or_else(|| "Unknown error".to_string());
            bail!("Telegram getUpdates error: {desc}");
        }

        Ok(api_res.result.unwrap_or_default())
    }
}
