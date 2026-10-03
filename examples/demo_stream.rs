use anyhow::Result;
use futures::StreamExt;
use reqwest::{Client, Response};
use serde_json::{json, Value};
use std::env;
use std::io::{self, Write};

fn print_banner(base_url: &str, model: &str, prompt: &str) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║             ⚡ DeeperSeeker Rust API Client Demo             ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!("  • Endpoint : {base_url}");
    println!("  • Model    : {model}");
    println!("  • Prompt   : \"{prompt}\"\n");
    print!("✦ Streaming Response:\n\n");
    let _ = io::stdout().flush();
}

fn handle_sse_line(line: &str) -> io::Result<bool> {
    let Some(data) = line.strip_prefix("data: ") else {
        return Ok(false);
    };
    if data == "[DONE]" {
        return Ok(true);
    }
    if let Ok(val) = serde_json::from_str::<Value>(data) {
        if let Some(content) = val["choices"][0]["delta"]["content"].as_str() {
            print!("{content}");
            io::stdout().flush()?;
        }
    }
    Ok(false)
}

async fn consume_stream(response: Response) -> Result<()> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk_result) = stream.next().await {
        let bytes = chunk_result?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if handle_sse_line(&line)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let base_url =
        env::var("DEEPERSEEKER_URL").unwrap_or_else(|_| "http://mochi:4000/v1".to_string());
    let api_key = env::var("DEEPERSEEKER_API_KEY").unwrap_or_else(|_| "dseeker".to_string());
    let model = env::var("DEEPERSEEKER_MODEL").unwrap_or_else(|_| "v4.1flash".to_string());
    let prompt = env::args()
        .nth(1)
        .unwrap_or_else(|| "Explain Axum extractors in 2 sentences.".to_string());

    print_banner(&base_url, &model, &prompt);

    let url = format!("{base_url}/chat/completions");
    let payload = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": true
    });

    let res = Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    consume_stream(res).await?;

    println!("\n\n────────────────────────────────────────────────────────────────");
    println!("✔ Completed stream successfully.");
    Ok(())
}
