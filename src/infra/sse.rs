pub struct ExtractedChunk {
    pub content: Option<String>,
    pub reasoning: Option<String>,
}

pub enum SseLineResult {
    Done,
    Chunks(Vec<ExtractedChunk>),
    None,
}

pub fn drain_sse_lines(buffer: &mut String, bytes: &[u8]) -> Vec<String> {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    let mut lines = Vec::new();
    while let Some(pos) = buffer.find('\n') {
        let line = buffer[..pos].trim().to_string();
        *buffer = buffer[pos + 1..].to_string();
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

pub fn parse_sse_line(line: &str, think_open: &mut bool) -> SseLineResult {
    let maybe_data = line
        .strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"));

    let Some(data) = maybe_data else {
        return SseLineResult::None;
    };

    if data == "[DONE]" {
        return SseLineResult::Done;
    }

    let Ok(json_val) = serde_json::from_str::<serde_json::Value>(data) else {
        return SseLineResult::None;
    };

    let chunks = extract_chunks_from_event(&json_val, think_open);
    SseLineResult::Chunks(chunks)
}

pub fn extract_chunks_from_event(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Vec<ExtractedChunk> {
    update_think_state(val, think_open);

    if is_status_event(val) {
        return Vec::new();
    }
    if let Some(chunks) = extract_batch_chunks(val, think_open) {
        return chunks;
    }
    if let Some(chunks) = extract_fragment_chunks(val, think_open) {
        return chunks;
    }
    extract_string_delta_chunks(val, *think_open)
}

fn update_think_state(val: &serde_json::Value, think_open: &mut bool) {
    let Some(p) = val.get("p").and_then(|p| p.as_str()) else {
        return;
    };
    if p == "response/fragments/0" {
        if let Some(type_val) = val
            .get("v")
            .and_then(|v| v.get("type"))
            .and_then(|t| t.as_str())
        {
            if type_val == "THINK" {
                *think_open = true;
            }
        }
    } else if p == "response/fragments/1" {
        *think_open = false;
    }
}

fn is_status_event(val: &serde_json::Value) -> bool {
    if val.get("v").and_then(|v| v.as_str()) == Some("FINISHED") {
        return true;
    }
    let Some(p) = val.get("p").and_then(|p| p.as_str()) else {
        return false;
    };
    p.contains("status")
        || p == "quasi_status"
        || p.contains("accumulated_token")
        || p.contains("view_state")
}

fn extract_batch_chunks(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Option<Vec<ExtractedChunk>> {
    if val.get("o").and_then(|o| o.as_str()) != Some("BATCH") {
        return None;
    }
    let v_arr = val.get("v").and_then(|v| v.as_array())?;

    let mut result = Vec::new();
    for item in v_arr {
        let sub_chunks = extract_chunks_from_event(item, think_open);
        result.extend(sub_chunks);
    }
    Some(result)
}

fn extract_fragment_chunks(
    val: &serde_json::Value,
    think_open: &mut bool,
) -> Option<Vec<ExtractedChunk>> {
    let fragments = extract_fragments(val)?;
    let mut result = Vec::new();

    for frag in fragments {
        let f_type = frag.get("type").and_then(|t| t.as_str());
        let f_content = frag.get("content").and_then(|c| c.as_str());

        match (f_type, f_content) {
            (Some("THINK"), Some(text)) => {
                *think_open = true;
                result.push(ExtractedChunk {
                    content: None,
                    reasoning: Some(text.to_string()),
                });
            }
            (Some("RESPONSE"), Some(text)) => {
                *think_open = false;
                result.push(ExtractedChunk {
                    content: Some(text.to_string()),
                    reasoning: None,
                });
            }
            _ => {}
        }
    }
    Some(result)
}

fn extract_string_delta_chunks(val: &serde_json::Value, think_open: bool) -> Vec<ExtractedChunk> {
    let Some(v_str) = val.get("v").and_then(|v| v.as_str()) else {
        return Vec::new();
    };
    if v_str.is_empty() {
        return Vec::new();
    }

    if think_open {
        vec![ExtractedChunk {
            content: None,
            reasoning: Some(v_str.to_string()),
        }]
    } else {
        vec![ExtractedChunk {
            content: Some(v_str.to_string()),
            reasoning: None,
        }]
    }
}

fn extract_fragments(val: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    if val.get("p").and_then(|p| p.as_str()) == Some("response/fragments") {
        return val.get("v").and_then(|v| v.as_array());
    }

    val.get("v")
        .and_then(|v| v.get("response"))
        .and_then(|r| r.get("fragments"))
        .and_then(|f| f.as_array())
}
