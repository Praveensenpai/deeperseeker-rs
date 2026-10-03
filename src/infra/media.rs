use crate::domain::openai::{ChatMessage, MessageContent};
use crate::infra::db::record_file;
use crate::infra::deepseek_client::DeepSeekClient;
use crate::infra::pow::PowSolver;
use crate::infra::rehome::rehome_foreign_files;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use std::sync::Arc;
use tokio_rusqlite::Connection;

pub struct MediaContext<'a> {
    pub db: &'a Connection,
    pub client: &'a DeepSeekClient,
    pub solver: &'a Arc<PowSolver>,
    pub token_id: i64,
    pub token: &'a str,
}

pub struct DecodedImage {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub filename: String,
}

pub async fn resolve_message_media(
    ctx: &MediaContext<'_>,
    messages: &[ChatMessage],
) -> Result<Vec<String>> {
    let mut existing_file_ids = Vec::new();
    let mut image_urls = Vec::new();

    for msg in messages {
        let MessageContent::Parts(parts) = &msg.content else {
            continue;
        };
        for part in parts {
            if let Some(f) = &part.file {
                existing_file_ids.push(f.file_id.clone());
            }
            if let Some(img) = &part.image_url {
                image_urls.push(img.url.clone());
            } else if let Some(url) = &part.url {
                image_urls.push(url.clone());
            } else if let Some(data) = &part.data {
                image_urls.push(data.clone());
            }
        }
    }

    let mut resolved_ids = Vec::new();

    if !existing_file_ids.is_empty() {
        let rehomed = rehome_foreign_files(
            ctx.db,
            ctx.client,
            ctx.solver,
            &existing_file_ids,
            ctx.token_id,
            ctx.token,
        )
        .await
        .unwrap_or(existing_file_ids);
        resolved_ids.extend(rehomed);
    }

    for url in image_urls {
        let decoded = parse_image_source(ctx.client, &url).await?;
        let file_id = upload_inline_image(ctx, decoded).await?;
        resolved_ids.push(file_id);
    }

    Ok(resolved_ids)
}

async fn upload_inline_image(ctx: &MediaContext<'_>, img: DecodedImage) -> Result<String> {
    let target_path = "/api/v0/file/upload_file";
    let challenge = ctx
        .client
        .create_pow_challenge(ctx.token, target_path)
        .await?;
    let pow_resp = ctx.solver.solve(&challenge, target_path)?;
    let file_id = ctx
        .client
        .upload_file(
            ctx.token,
            &pow_resp,
            &img.filename,
            &img.mime_type,
            img.bytes,
        )
        .await?;
    let _ = record_file(ctx.db, &file_id, ctx.token_id).await;
    Ok(file_id)
}

pub async fn parse_image_source(client: &DeepSeekClient, url: &str) -> Result<DecodedImage> {
    if url.starts_with("data:") {
        parse_data_uri(url)
    } else if url.starts_with("http://") || url.starts_with("https://") {
        parse_remote_url(client, url).await
    } else {
        parse_raw_base64(url)
    }
}

fn parse_data_uri(url: &str) -> Result<DecodedImage> {
    let (header, data_str) = url
        .split_once(',')
        .ok_or_else(|| anyhow!("Invalid data URI: missing comma separator"))?;

    let is_base64 = header.contains(";base64");
    let bytes = if is_base64 {
        decode_base64_string(data_str)?
    } else {
        data_str.as_bytes().to_vec()
    };

    let (mime, ext) = detect_image_format(&bytes);
    let filename = format!("upload_{}.{}", uuid::Uuid::new_v4(), ext);
    Ok(DecodedImage {
        bytes,
        mime_type: mime.to_string(),
        filename,
    })
}

async fn parse_remote_url(client: &DeepSeekClient, url: &str) -> Result<DecodedImage> {
    let bytes = client.fetch_bytes(url).await?;
    let (mime, ext) = detect_image_format(&bytes);
    let filename = format!("upload_{}.{}", uuid::Uuid::new_v4(), ext);
    Ok(DecodedImage {
        bytes,
        mime_type: mime.to_string(),
        filename,
    })
}

fn parse_raw_base64(raw: &str) -> Result<DecodedImage> {
    let bytes = decode_base64_string(raw)?;
    let (mime, ext) = detect_image_format(&bytes);
    let filename = format!("upload_{}.{}", uuid::Uuid::new_v4(), ext);
    Ok(DecodedImage {
        bytes,
        mime_type: mime.to_string(),
        filename,
    })
}

fn decode_base64_string(s: &str) -> Result<Vec<u8>> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    B64.decode(clean.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(clean.as_bytes()))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(clean.as_bytes()))
        .context("Failed to decode base64 image data")
}

pub fn detect_image_format(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        ("image/png", "png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("image/jpeg", "jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        ("image/gif", "gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        ("image/webp", "webp")
    } else {
        ("image/png", "png")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_image_formats() {
        assert_eq!(
            detect_image_format(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
            ("image/png", "png")
        );
        assert_eq!(
            detect_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            ("image/jpeg", "jpg")
        );
        assert_eq!(detect_image_format(b"GIF89a"), ("image/gif", "gif"));
        let webp_bytes = [b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'W', b'E', b'B', b'P'];
        assert_eq!(detect_image_format(&webp_bytes), ("image/webp", "webp"));
    }

    #[test]
    fn test_parse_data_uri() {
        let sample = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let decoded = parse_data_uri(sample).expect("parse valid png data uri");
        assert_eq!(decoded.mime_type, "image/png");
        assert!(decoded.filename.ends_with(".png"));
        assert!(!decoded.bytes.is_empty());
    }

    #[test]
    fn test_parse_raw_base64() {
        let sample = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let decoded = parse_raw_base64(sample).expect("parse raw base64");
        assert_eq!(decoded.mime_type, "image/png");
        assert!(decoded.filename.ends_with(".png"));
    }
}
