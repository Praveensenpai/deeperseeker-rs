use crate::domain::openai::{FunctionCall, ToolCall};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use uuid::Uuid;

static CALLS_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<\s*[｜\|\s]*(?:DSML[\s｜\|\s]*)?(?:calls|tool_calls)[^>]*>(.*?)(?:<\s*/[\s｜\|\s]*(?:DSML[\s｜\|\s]*)?(?:calls|tool_calls)[^>]*>|$)")
        .expect("valid regex")
});

static INVOKE_BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<\s*[｜\|\s]*(?:DSML[\s｜\|\s]*)?invoke\b([^>]*)>(.*?)(?:<\s*/[\s｜\|\s]*(?:DSML[\s｜\|\s]*)?invoke[^>]*>|$)")
        .expect("valid regex")
});

static PARAM_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?is)<?\s*[/｜\|\s]*(?:DSML[\s｜\|\s]*)?parameter\b([^>]*)>"#)
        .expect("valid regex")
});

static HERMES_FUNCTION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<\s*function=([a-zA-Z0-9_\-]+)>(.*?)(?:<\s*/function>|$)")
        .expect("valid regex")
});

static HERMES_PARAM_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<\s*parameter=([a-zA-Z0-9_\-]+)>(.*?)(?:<\s*/parameter>|$)")
        .expect("valid regex")
});

static ATTR_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\bname\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

static ATTR_STRING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?i)\bstring\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

static STRIP_BLOCKS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<\s*tool_call>.*?(?:<\s*/(?:tool_)?call>|$)|<\s*[/｜\|\s]*(?:DSML[\s｜\|\s]*)?invoke\b[^>]*>.*?(?:<\s*/[\s｜\|\s]*(?:DSML[\s｜\|\s]*)?invoke>|$)|<\s*function_call>.*?(?:<\s*/function_call>|$)")
        .expect("valid regex")
});

static STRIP_TAGS_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?is)<?\s*/?[\s｜\|\s]*(?:DSML[\s｜\|\s]*)?(?:tool_calls?|calls|function_calls?|invoke|parameter|tool_call|function_call|content|function|call)\b[^>]*>|FINISHED$")
        .expect("valid regex")
});

static DSML_START_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)<\s*[｜\|\s]*(?:dsml|invoke|tool_calls?|calls|function_calls?)\b|<\s*(?:tool_call|function_call)\b|<\s*function=|<\s*call\b")
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
        "dsml",
        "invoke",
        "tool_call",
        "tool_calls",
        "function_call",
        "function",
        "calls",
        "parameter",
        "call",
    ];
    PREFIXES
        .iter()
        .any(|p| p.starts_with(trimmed) || trimmed.starts_with(p))
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
    let cleaned = if !tool_calls.is_empty() {
        if let Some(pos) = find_dsml_block_start(text) {
            text[..pos].trim().to_string()
        } else {
            let without_blocks = STRIP_BLOCKS_RE.replace_all(text, "");
            STRIP_TAGS_RE
                .replace_all(&without_blocks, "")
                .trim()
                .to_string()
        }
    } else {
        let without_blocks = STRIP_BLOCKS_RE.replace_all(text, "");
        STRIP_TAGS_RE
            .replace_all(&without_blocks, "")
            .trim()
            .to_string()
    };

    ParsedDsml {
        text_content: cleaned,
        tool_calls,
    }
}

pub fn clean_history_text(text: &str) -> String {
    let without_blocks = STRIP_BLOCKS_RE.replace_all(text, "");
    STRIP_TAGS_RE
        .replace_all(&without_blocks, "")
        .trim()
        .to_string()
}

fn extract_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut positioned_calls = Vec::new();
    let session_prefix = Uuid::new_v4().simple().to_string();

    extract_dsml_invokes(text, &session_prefix, &mut positioned_calls);
    extract_hermes_tool_calls(text, &session_prefix, &mut positioned_calls);
    extract_json_tool_calls(text, &session_prefix, &mut positioned_calls);

    positioned_calls.sort_by_key(|(pos, _)| *pos);

    let mut unique_calls = Vec::new();
    for (_, tc) in positioned_calls {
        let is_dup = unique_calls.iter().any(|u: &ToolCall| {
            u.function.name == tc.function.name && u.function.arguments == tc.function.arguments
        });
        if !is_dup {
            unique_calls.push(tc);
        }
    }
    for (i, tc) in unique_calls.iter_mut().enumerate() {
        tc.index = Some(i);
        tc.id = format!(
            "call_{}_{i}",
            &session_prefix[..8.min(session_prefix.len())]
        );
    }
    unique_calls
}

fn extract_dsml_invokes(text: &str, session_prefix: &str, tool_calls: &mut Vec<(usize, ToolCall)>) {
    let body_text = if let Some(cap) = CALLS_BLOCK_RE.captures(text) {
        cap.get(1).map(|m| m.as_str()).unwrap_or(text)
    } else {
        text
    };

    for (idx, cap) in INVOKE_BLOCK_RE.captures_iter(body_text).enumerate() {
        let full_match = cap.get(0).unwrap();
        let pos = full_match.start();
        let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(name_match) = ATTR_NAME_RE.captures(attrs).and_then(|c| c.get(1)) else {
            continue;
        };
        let tool_name = name_match.as_str().to_string();
        let invoke_body = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let arguments = extract_arguments(invoke_body);

        tool_calls.push((
            pos,
            ToolCall {
                index: Some(idx),
                id: format!("call_{}_{idx}", &session_prefix[..8]),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: tool_name,
                    arguments,
                },
            },
        ));
    }
}

fn extract_hermes_tool_calls(
    text: &str,
    session_prefix: &str,
    tool_calls: &mut Vec<(usize, ToolCall)>,
) {
    for (idx, cap) in HERMES_FUNCTION_RE.captures_iter(text).enumerate() {
        let full_match = cap.get(0).unwrap();
        let pos = full_match.start();
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
        tool_calls.push((
            pos,
            ToolCall {
                index: Some(idx),
                id: format!("call_{}_{idx}", &session_prefix[..8]),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: tool_name,
                    arguments,
                },
            },
        ));
    }
}

fn extract_json_tool_calls(
    text: &str,
    session_prefix: &str,
    tool_calls: &mut Vec<(usize, ToolCall)>,
) {
    let mut search_idx = 0;
    while let Some(start_pos) = text[search_idx..].find("<tool_call>") {
        let abs_start = search_idx + start_pos;
        let tag_end = abs_start + "<tool_call>".len();
        let slice = &text[tag_end..];
        let Some(brace_rel) = slice.find('{') else {
            search_idx = tag_end;
            continue;
        };
        let json_start = tag_end + brace_rel;
        let candidate = &text[json_start..];
        let mut parsed_val = None;
        let mut consumed_bytes = 0;

        let mut de = serde_json::Deserializer::from_str(candidate).into_iter::<Value>();
        if let Some(Ok(val)) = de.next() {
            consumed_bytes = de.byte_offset();
            parsed_val = Some(val);
        } else {
            let end_limit = candidate
                .find("</tool_call>")
                .or_else(|| candidate.find("</call>"))
                .or_else(|| candidate.find('<'))
                .unwrap_or(candidate.len());
            let sub = candidate[..end_limit].trim_end();
            let open_braces = sub.chars().filter(|&c| c == '{').count();
            let close_braces = sub.chars().filter(|&c| c == '}').count();
            if open_braces > close_braces {
                let repaired = format!("{}{}", sub, "}".repeat(open_braces - close_braces));
                if let Ok(val) = serde_json::from_str::<Value>(&repaired) {
                    consumed_bytes = sub.len();
                    parsed_val = Some(val);
                }
            }
        }

        if let Some(val) = parsed_val {
            search_idx = json_start + consumed_bytes;
            let Some(name) = val
                .get("name")
                .or_else(|| val.get("tool"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let args_val = val.get("arguments").or_else(|| val.get("parameters"));
            let arguments = match args_val {
                Some(Value::String(s)) => s.clone(),
                Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
                None => "{}".to_string(),
            };

            let prefix_len = 8.min(session_prefix.len());
            tool_calls.push((
                abs_start,
                ToolCall {
                    index: Some(tool_calls.len()),
                    id: format!(
                        "call_{}_{}",
                        &session_prefix[..prefix_len],
                        tool_calls.len()
                    ),
                    r#type: "function".to_string(),
                    function: FunctionCall {
                        name: name.to_string(),
                        arguments,
                    },
                },
            ));
        } else {
            search_idx = tag_end;
        }
    }
}

fn extract_arguments(invoke_body: &str) -> String {
    let mut args_map = serde_json::Map::new();
    let matches: Vec<_> = PARAM_TAG_RE.find_iter(invoke_body).collect();

    if !matches.is_empty() {
        for (i, m) in matches.iter().enumerate() {
            let cap = PARAM_TAG_RE.captures(m.as_str()).unwrap();
            let attrs = cap.get(1).map(|c| c.as_str()).unwrap_or("");
            let Some(name_match) = ATTR_NAME_RE.captures(attrs).and_then(|c| c.get(1)) else {
                continue;
            };
            let param_name = name_match.as_str();
            let is_str = ATTR_STRING_RE
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                != Some("false");

            let val_start = m.end();
            let val_end = if i + 1 < matches.len() {
                matches[i + 1].start()
            } else {
                invoke_body.len()
            };
            let raw_val = &invoke_body[val_start..val_end];
            let clean_val = STRIP_TAGS_RE.replace_all(raw_val, "").trim().to_string();
            insert_param_value(&mut args_map, param_name, &clean_val, is_str);
        }
        if args_map.len() == 1 {
            if let Some(val) = args_map
                .get("arguments")
                .or_else(|| args_map.get("parameters"))
            {
                if let Value::Object(_) = val {
                    return serde_json::to_string(val).unwrap_or_else(|_| "{}".to_string());
                } else if let Value::String(s) = val {
                    if s.trim().starts_with('{') && s.trim().ends_with('}') {
                        return s.trim().to_string();
                    }
                }
            }
        }
        return serde_json::to_string(&args_map).unwrap_or_else(|_| "{}".to_string());
    }

    let trimmed = invoke_body.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return trimmed.to_string();
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
        assert_eq!(
            parsed.tool_calls[0].function.arguments,
            r#"{"command":"cargo check"}"#
        );
    }

    #[test]
    fn test_parse_json_tool_call_with_nested_objects_and_code() {
        let raw = "Here is the edit.\n<tool_call>{\"name\": \"edit\", \"arguments\": {\"filePath\": \"src/services.py\", \"oldString\": \"def foo(): { return 1; }\", \"newString\": \"def bar(): { return 2; }\"}}</｜｜DSML｜｜ calls>";
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.text_content, "Here is the edit.");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].function.name, "edit");
        assert!(parsed.tool_calls[0]
            .function
            .arguments
            .contains("src/services.py"));
        assert!(parsed.tool_calls[0]
            .function
            .arguments
            .contains("def foo(): { return 1; }"));
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

    #[test]
    fn test_parse_dsml_malformed_parameter_tags() {
        let raw = r#"<｜DSML｜invoke name="edit"><｜DSML｜parameter name="filePath" string="true">src/services.py</｜｜DSML｜｜ parameter name="oldString" string="true">def old(): pass</｜｜DSML｜｜ parameter name="newString" string="true">def new(): pass</｜｜DSML｜｜ invoke>"#;
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.tool_calls.len(), 1);
        let call = &parsed.tool_calls[0];
        assert_eq!(call.function.name, "edit");
        assert!(call
            .function
            .arguments
            .contains(r#""filePath":"src/services.py""#));
        assert!(call
            .function
            .arguments
            .contains(r#""oldString":"def old(): pass""#));
        assert!(call
            .function
            .arguments
            .contains(r#""newString":"def new(): pass""#));
    }

    #[test]
    fn test_parse_hybrid_spaced_dsml_leak() {
        let raw = r#"Regression test passes.

<tool_call>{"name": "shell", "arguments": {"command": "curl http://localhost:8096"}
< / | DSML | | parameter>
< / | DSML | | invoke>
< | | DSML | | invoke name="shell" string="false">true< / | DSML | | parameter>
< | | DSML | | parameter name="arguments">{"command": "cargo build --release"}
< / | DSML | | invoke>
< / | DSML | | calls>"#;
        let parsed = parse_dsml(raw);
        assert_eq!(parsed.text_content, "Regression test passes.");
        assert_eq!(parsed.tool_calls.len(), 2);
        assert_eq!(parsed.tool_calls[0].function.name, "shell");
        assert!(parsed.tool_calls[0]
            .function
            .arguments
            .contains("curl http://localhost:8096"));
        assert_eq!(parsed.tool_calls[1].function.name, "shell");
        assert!(parsed.tool_calls[1]
            .function
            .arguments
            .contains("cargo build --release"));
    }
}
