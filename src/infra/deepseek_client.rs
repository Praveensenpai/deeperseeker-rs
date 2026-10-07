use crate::domain::upstream::{CreateChatResponse, PowChallenge, PowChallengeWrapper};
use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

const BASE_URL: &str = "https://chat.deepseek.com";
const CLIENT_VERSION: &str = "2.4.5";

#[derive(Clone)]
pub struct DeepSeekClient {
    client: Client,
    base_url: String,
}

pub struct CompletionArgs {
    pub token: String,
    pub session_id: String,
    pub parent_message_id: Option<i64>,
    pub prompt: String,
    pub pow_response: String,
    pub ref_file_ids: Vec<String>,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
}

impl Default for DeepSeekClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepSeekClient {
    /// Default client: 300s idle-read timeout, 15s connect timeout.
    pub fn new() -> Self {
        Self::with_timeouts(300, 15)
    }

    /// Build a client with explicit idle-read and connect timeouts.
    ///
    /// A build failure is logged rather than silently swallowed; the fallback
    /// is a dependency-default client so the proxy still starts.
    pub fn with_timeouts(timeout_secs: u64, connect_timeout_secs: u64) -> Self {
        Self::with_base_url(BASE_URL, timeout_secs, connect_timeout_secs)
    }

    /// Build a client against an explicit upstream base URL. The trailing
    /// slash is trimmed so path joining stays consistent. Used by tests to
    /// point at a mock upstream.
    pub fn with_base_url(base_url: &str, timeout_secs: u64, connect_timeout_secs: u64) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        match Client::builder()
            .tcp_keepalive(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(connect_timeout_secs.max(1)))
            .read_timeout(Duration::from_secs(timeout_secs.max(1)))
            .build()
        {
            Ok(client) => Self { client, base_url },
            Err(e) => {
                tracing::warn!(
                    "Failed to build HTTP client with timeouts ({e}); \
                     falling back to default client"
                );
                Self {
                    client: Client::new(),
                    base_url,
                }
            }
        }
    }

    /// Resolved upstream base URL (no trailing slash).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn build_headers(&self, token: &str, pow: Option<&str>) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("Dalvik/2.1.0 (Linux; U; Android 14; Pixel 7)"),
        );
        headers.insert("x-client-platform", HeaderValue::from_static("android"));
        headers.insert("x-client-version", HeaderValue::from_static(CLIENT_VERSION));
        headers.insert("x-client-locale", HeaderValue::from_static("en_US"));
        headers.insert(
            "x-client-bundle-id",
            HeaderValue::from_static("com.deepseek.chat"),
        );
        headers.insert(
            "origin",
            HeaderValue::from_static("https://chat.deepseek.com"),
        );
        headers.insert(
            "referer",
            HeaderValue::from_static("https://chat.deepseek.com/"),
        );

        let auth = format!("Bearer {token}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth).context("Invalid auth header")?,
        );

        if let Some(p) = pow {
            headers.insert(
                "x-ds-pow-response",
                HeaderValue::from_str(p).context("Invalid pow header")?,
            );
        }

        Ok(headers)
    }

    pub async fn create_pow_challenge(
        &self,
        token: &str,
        target_path: &str,
    ) -> Result<PowChallenge> {
        let url = format!("{}/api/v0/chat/create_pow_challenge", self.base_url);
        let headers = self.build_headers(token, None)?;
        let body = json!({ "target_path": target_path });

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("PoW challenge request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("PoW challenge HTTP {status}: {text}"));
        }

        let wrapper: PowChallengeWrapper = resp
            .json()
            .await
            .context("Failed to parse PoW challenge response")?;

        let data = wrapper
            .extract_data()
            .context("PoW challenge rejected by upstream")?;

        Ok(data.biz_data.challenge)
    }

    pub async fn create_chat_session(&self, token: &str) -> Result<String> {
        let url = format!("{}/api/v0/chat_session/create", self.base_url);
        let headers = self.build_headers(token, None)?;
        let body = json!({ "character_id": null });

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Create chat session request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Create chat session HTTP {status}: {text}"));
        }

        let wrapper: CreateChatResponse = resp
            .json()
            .await
            .context("Failed to parse create chat response")?;

        let data = wrapper
            .extract_data()
            .context("Create chat session rejected by upstream")?;

        Ok(data.biz_data.chat_session.id)
    }

    pub async fn send_completion_request(&self, args: CompletionArgs) -> Result<reqwest::Response> {
        let url = format!("{}/api/v0/chat/completion", self.base_url);
        let headers = self.build_headers(&args.token, Some(&args.pow_response))?;

        let body = json!({
            "chat_session_id": args.session_id,
            "parent_message_id": args.parent_message_id,
            "model_type": "deepseek_chat",
            "prompt": args.prompt,
            "ref_file_ids": args.ref_file_ids,
            "thinking_enabled": args.thinking_enabled,
            "search_enabled": args.search_enabled,
            "preempt": false,
            "action": null
        });

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Completion stream request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("DeepSeek upstream HTTP {status}: {text}"));
        }

        let is_sse = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|ct| ct.to_str().ok())
            .map(|s| s.contains("text/event-stream"))
            .unwrap_or(true);

        if !is_sse {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "DeepSeek upstream error (non-SSE response): {text}"
            ));
        }

        Ok(resp)
    }

    pub async fn upload_file(
        &self,
        token: &str,
        pow_resp: &str,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<String> {
        let url = format!("{}/api/v0/file/upload_file", self.base_url);
        let mut headers = self.build_headers(token, Some(pow_resp))?;
        headers.remove(CONTENT_TYPE);

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(content_type)?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .multipart(form)
            .send()
            .await
            .context("File upload request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("File upload HTTP {status}: {text}"));
        }

        let json_val: serde_json::Value = resp.json().await?;
        let file_id = json_val
            .get("data")
            .and_then(|d| d.get("biz_data"))
            .and_then(|b| b.get("id"))
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow!("Missing file id in upload response"))?;

        Ok(file_id.to_string())
    }

    pub async fn download_file(&self, token: &str, file_id: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/api/v0/file/download_file?file_id={file_id}",
            self.base_url
        );
        let headers = self.build_headers(token, None)?;

        let resp = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .context("File download request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(anyhow!("File download HTTP {status}"));
        }

        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed fetching remote URL")?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(anyhow!("Remote URL HTTP {status}"));
        }

        let bytes = resp
            .bytes()
            .await
            .context("Failed reading response bytes")?;
        Ok(bytes.to_vec())
    }
}
