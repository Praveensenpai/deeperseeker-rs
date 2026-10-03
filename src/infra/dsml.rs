use crate::domain::openai::{FunctionCall, ToolCall};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use uuid::Uuid;

static CALLS_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<[｜\|\s]*DSML[｜\|\s]*(?:calls|tool_calls)[^>]*>(.*?)(?:</[｜\|\s]*DSML[｜\|\s]*(?:calls|tool_calls)>|$)")
        .expect("valid regex")
});

static INVOKE_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<[｜\|\s]*(?:DSML[｜\|\s]*)?invoke\b([^>]*)>(.*?)(?:</[｜\|\s]*(?:DSML[｜\|\s]*)?invoke>|$)")
        .expect("valid regex")
});

static PARAM_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<[｜\|\s]*(?:DSML[｜\|\s]*)?parameter\b([^>]*)>(.*?)(?:</[｜\|\s]*(?:DSML[｜\|\s]*)?parameter>|$)")
        .expect("valid regex")
});

static HERMES_FUNCTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<function=([a-zA-Z0-9_\-]+)>(.*?)(?:</function>|$)")
        .expect("valid regex")
});

static HERMES_PARAM_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<parameter=([a-zA-Z0-9_\-]+)>(.*?)(?:</parameter>|$)")
        .expect("valid regex")
});

static JSON_TOOL_CALL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<tool_call>\s*(\{.*?\})\s*(?:</(?:tool_)?call>|$)")
        .expect("valid regex")
});

static ATTR_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\bname\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

static ATTR_STRING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\bstring\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

static STRIP_BLOCKS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<tool_call>.*?(?:</(?:tool_)?call>|$)|<[｜\|\s]*(?:DSML[｜\|\s]*)?invoke\b[^>]*>.*?(?:</[｜\|\s]*(?:DSML[｜\|\s]*)?invoke>|$)|<function_call>.*?(?:</function_call>|$)")
        .expect("valid regex")
});

static STRIP_TAGS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)</?[｜\|\s]*(?:DSML[｜\|\s]*)?(?:tool_calls?|calls|function_calls?|invoke|parameter|tool_call|function_call|content|function|call)\b[^>]*>|FINISHED$")
        .expect("valid regex")
});

static DSML_START_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<[｜\|\s]*(?:dsml|invoke|tool_calls?|calls|function_calls?)\b|<(?:tool_call|function_call)\b|<function=|<call\b")
        .expect("valid regex")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedDsml {
    pub text_content: String,
    pub tool_calls: Vec<ToolCall>,
}

pub fn find_dsml_block_start(text: &str) -> Option<usize> {
    DSML_START_RE.find(text).map(|m| m.start())
}

pub fn safe_unambiguous_len(text: &str) -> usize {
    let mut search_start = text.len().saturating_sub(32);
    while search_start > 0 && !text.is_char_boundary(search_start) {
        search_start -= 1;
    }
    if let Some(rel_pos) = text[search_start..].rfind('<') {
        let tag_start = search_start + rel_pos;
        let suffix = &text[tag_start + 1..];
        if is_potential_dsml_prefix(suffix) {
            return tag_start;
        }
    }
    text.len()
}

fn is_potential_dsml_prefix(suffix: &str) -> bool {
    let lower = suffix.to_lowercase();
    let trimmed = lower.trim_start_matches(['|', '\u{ff5c}', ' ', '/']);
    if trimmed.is_empty() {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "dsml", "invoke", "tool_call", "tool_calls", "function_call", "function", "calls", "parameter", "call",
    ];
    PREFIXES.iter().any(|p| p.starts_with(trimmed) || trimmed.starts_with(p))
}

pub fn parse_dsml(text: &str) -> ParsedDsml {
    let has_markers = text.contains("DSML")
        || text.contains("dsml")
        || text.contains("invoke")
        || text.contains("tool_call")
        || text.contains("function=")
        || text.contains("function_call")
        || text.contains("</call>");

    if !has_markers {
        return ParsedDsml {
            text_content: text.trim().to_string(),
            tool_calls: Vec::new(),
        };
    }

    let tool_calls = extract_tool_calls(text);
    let without_blocks = STRIP_BLOCKS_RE.replace_all(text, "");
    let cleaned = STRIP_TAGS_RE.replace_all(&without_blocks, "").trim().to_string();

    ParsedDsml {
        text_content: cleaned,
        tool_calls,
    }
}

fn extract_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();
    let session_prefix = Uuid::new_v4().simple().to_string();

    extract_dsml_invokes(text, &session_prefix, &mut tool_calls);
    if tool_calls.is_empty() {
        extract_hermes_tool_calls(text, &session_prefix, &mut tool_calls);
    }
    if tool_calls.is_empty() {
        extract_json_tool_calls(text, &session_prefix, &mut tool_calls);
    }

    tool_calls
}

fn extract_dsml_invokes(text: &str, session_prefix: &str, tool_calls: &mut Vec<ToolCall>) {
    let body_text = if let Some(cap) = CALLS_BLOCK_RE.captures(text) {
        cap.get(1).map(|m| m.as_str()).unwrap_or(text)
    } else {
        text
    };

    for (idx, cap) in INVOKE_BLOCK_RE.captures_iter(body_text).enumerate() {
        let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(name_match) = ATTR_NAME_RE.captures(attrs).and_then(|c| c.get(1)) else {
            continue;
        };
        let tool_name = name_match.as_str().to_string();
        let invoke_body = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let arguments = extract_arguments(invoke_body);

        tool_calls.push(ToolCall {
            index: Some(idx),
            id: format!("call_{}_{idx}", &session_prefix[..8]),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: tool_name,
                arguments,
            },
        });
    }
}

fn extract_hermes_tool_calls(text: &str, session_prefix: &str, tool_calls: &mut Vec<ToolCall>) {
    for (idx, cap) in HERMES_FUNCTION_RE.captures_iter(text).enumerate() {
        let Some(name_match) = cap.get(1) else {
            continue;
        };
        let tool_name = name_match.as_str().to_string();
        let body = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let mut args_map = serde_json::Map::new();

        for p_cap in HERMES_PARAM_RE.captures_iter(body) {
            let Some(param_name) = p_cap.get(1).map(|m| m.as_str()) else {
                continue;
            };
            let param_val = p_cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            insert_param_value(&mut args_map, param_name, param_val, true);
        }

        let arguments = serde_json::to_string(&args_map).unwrap_or_else(|_| "{}".to_string());
        tool_calls.push(ToolCall {
            index: Some(idx),
            id: format!("call_{}_{idx}", &session_prefix[..8]),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: tool_name,
                arguments,
            },
        });
    }
}

fn extract_json_tool_calls(text: &str, session_prefix: &str, tool_calls: &mut Vec<ToolCall>) {
    for (idx, cap) in JSON_TOOL_CALL_RE.captures_iter(text).enumerate() {
        let Some(json_str) = cap.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(json_str) else {
            continue;
        };
        let Some(name) = val.get("name").or_else(|| val.get("tool")).and_then(|v| v.as_str()) else {
            continue;
        };
        let args_val = val.get("arguments").or_else(|| val.get("parameters"));
        let arguments = match args_val {
            Some(Value::String(s)) => s.clone(),
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            None => "{}".to_string(),
        };

        tool_calls.push(ToolCall {
            index: Some(idx),
            id: format!("call_{}_{idx}", &session_prefix[..8]),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments,
            },
        });
    }
}

fn extract_arguments(invoke_body: &str) -> String {
    let mut args_map = serde_json::Map::new();
    let mut has_params = false;

    for p_cap in PARAM_BLOCK_RE.captures_iter(invoke_body) {
        has_params = true;
        let attrs = p_cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(param_name) = ATTR_NAME_RE
            .captures(attrs)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
        else {
            continue;
        };
        let is_str = ATTR_STRING_RE
            .captures(attrs)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            != Some("false");
        let param_val = p_cap.get(2).map(|m| m.as_str().trim()).unwrap_or("");

        insert_param_value(&mut args_map, param_name, param_val, is_str);
    }

    if !has_params {
        let trimmed = invoke_body.trim();
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            return trimmed.to_string();
        }
    }

    serde_json::to_string(&args_map).unwrap_or_else(|_| "{}".to_string())
}

fn insert_param_value(
    map: &mut serde_json::Map<String, Value>,
    name: &str,
    val: &str,
    is_str: bool,
) {
    if is_str {
        map.insert(name.to_string(), Value::String(val.to_string()));
    } else if let Ok(parsed_json) = serde_json::from_str::<Value>(val) {
        map.insert(name.to_string(), parsed_json);
    } else {
        map.insert(name.to_string(), Value::String(val.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dsml_fullwidth_bash() {
        let raw = "I'll explore your projects directory.\n\n<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"bash\">\n<｜｜DSML｜｜ parameter name=\"command\" string=\"true\">ls -la ~/Projects</｜｜DSML｜｜ parameter>\n</｜｜DSML｜｜ invoke>\n</｜｜DSML｜｜ calls>FINISHED";
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.text_content, "I'll explore your projects directory.");
        assert_eq!(parsed.tool_calls.len(), 1);
        let call = &parsed.tool_calls[0];
        assert_eq!(call.function.name, "bash");
        assert_eq!(
            call.function.arguments,
            r#"{"command":"ls -la ~/Projects"}"#
        );
    }

    #[test]
    fn test_parse_dsml_multiple_invokes() {
        let raw = "<｜DSML｜calls>\n<｜DSML｜invoke name=\"read_file\">\n<｜DSML｜parameter name=\"path\" string=\"true\">/foo.txt</｜DSML｜parameter>\n</｜DSML｜invoke>\n<｜DSML｜invoke name=\"write_file\">\n<｜DSML｜parameter name=\"path\" string=\"true\">/bar.txt</｜DSML｜parameter>\n</｜DSML｜invoke>\n</｜DSML｜calls>";
        let parsed = parse_dsml(raw);
        assert!(parsed.text_content.is_empty());
        assert_eq!(parsed.tool_calls.len(), 2);
        assert_eq!(parsed.tool_calls[0].function.name, "read_file");
        assert_eq!(parsed.tool_calls[1].function.name, "write_file");
    }

    #[test]
    fn test_parse_dsml_non_string_param() {
        let raw =
            r#"<invoke name="test"><parameter name="count" string="false">42</parameter></invoke>"#;
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.arguments, r#"{"count":42}"#);
    }

    #[test]
    fn test_safe_unambiguous_len() {
        assert_eq!(safe_unambiguous_len("hello world"), 11);
        assert_eq!(safe_unambiguous_len("hello <"), 6);
        assert_eq!(safe_unambiguous_len("hello <｜"), 6);
        assert_eq!(safe_unambiguous_len("hello <｜｜DSML"), 6);
        assert_eq!(safe_unambiguous_len("5 < 10"), 6);
    }

    #[test]
    fn test_safe_unambiguous_len_multibyte_utf8() {
        // em-dashes, en-dashes, and emojis near boundary
        let sample = "anime4k-cli – Rust tool that manages Anime4K GLSL shaders for mpv.\ndubstrip — Lossless audio-stream stri 🦀";
        let len = safe_unambiguous_len(sample);
        assert_eq!(len, sample.len());
    }

    #[test]
    fn test_find_dsml_block_start() {
        assert_eq!(find_dsml_block_start("Hello <｜｜DSML｜｜ calls>"), Some(6));
        assert_eq!(find_dsml_block_start("<|dsml|calls>"), Some(0));
        assert_eq!(find_dsml_block_start("Let's edit <tool_call>"), Some(11));
        assert_eq!(find_dsml_block_start("Plain text"), None);
    }

    #[test]
    fn test_parse_hermes_tool_call() {
        let raw = "I'll update the configuration file.\n<tool_call>\n<function=edit>\n<parameter=filePath>/tmp/config.rs</parameter>\n<parameter=oldString>timeout: 5</parameter>\n<parameter=newString>timeout: 10</parameter>\n</function>\n</tool_call>";
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.text_content, "I'll update the configuration file.");
        assert_eq!(parsed.tool_calls.len(), 1);
        let call = &parsed.tool_calls[0];
        assert_eq!(call.function.name, "edit");
        assert_eq!(
            call.function.arguments,
            r#"{"filePath":"/tmp/config.rs","newString":"timeout: 10","oldString":"timeout: 5"}"#
        );
    }

    #[test]
    fn test_parse_json_tool_call() {
        let raw = "<tool_call>{\"name\": \"bash\", \"arguments\": {\"command\": \"cargo check\"}}</tool_call>";
        let parsed = parse_dsml(raw);
        assert!(parsed.text_content.is_empty());
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "bash");
        assert_eq!(parsed.tool_calls[0].function.arguments, r#"{"command":"cargo check"}"#);
    }

    #[test]
    fn test_parse_multiple_json_tool_calls_mixed_tags() {
        let raw = "Reading files.\n<tool_call>{\"name\": \"read\", \"arguments\": {\"filePath\": \"a.py\"}}</tool_call>\n<tool_call>{\"name\": \"read\", \"arguments\": {\"filePath\": \"b.py\"}}</call>\n</｜DSML｜ invoke>";
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.text_content, "Reading files.");
        assert_eq!(parsed.tool_calls.len(), 2);
        assert_eq!(parsed.tool_calls[0].function.name, "read");
        assert_eq!(parsed.tool_calls[1].function.name, "read");
    }
}
