use crate::domain::openai::ChatMessage;
use serde_json::Value;

pub const TOOL_USE_INSTRUCTIONS: &str = "\
TOOL USE INSTRUCTIONS:\n\
You have access to tools. When you need to call a tool, output tool call XML blocks:\n\
<tool_call>{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}</tool_call>\n\
Never repeat past messages or history. You may output multiple <tool_call>...</tool_call> blocks to execute tools in parallel.";

pub fn build_prompt_for_turn(
    messages: &[ChatMessage],
    tools: Option<&[Value]>,
    is_first: bool,
) -> String {
    if is_first {
        build_first_turn_prompt(messages, tools)
    } else {
        build_continuing_prompt(messages, tools)
    }
}

pub fn format_tools_section(tools: &[Value]) -> Option<String> {
    if tools.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for tool in tools {
        if let Some(fn_obj) = tool.get("function") {
            let name = fn_obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let desc = fn_obj.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let params = fn_obj.get("parameters").cloned().unwrap_or(serde_json::json!({}));
            parts.push(format!("Tool: {name}\nDescription: {desc}\nParameters: {params}"));
        } else if let Some(name) = tool.get("name").and_then(|v| v.as_str()) {
            let desc = tool.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let params = tool
                .get("input_schema")
                .or_else(|| tool.get("parameters"))
                .cloned()
                .unwrap_or(serde_json::json!({}));
            parts.push(format!("Tool: {name}\nDescription: {desc}\nParameters: {params}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn build_first_turn_prompt(messages: &[ChatMessage], tools: Option<&[Value]>) -> String {
    let mut prompt = String::new();
    let tools_text = tools.and_then(format_tools_section);

    if let Some(ref tt) = tools_text {
        prompt.push_str(&format!("[TOOLS]\n{tt}\n\n"));
    }

    if let Some(sys) = extract_system_prompt(messages, tools_text.is_some()) {
        prompt.push_str(&format!("[SYSTEM PROMPT]\n{sys}\n\n"));
    }

    if messages.len() > 1 {
        let history = build_conversation_history(messages);
        if !history.is_empty() {
            prompt.push_str(&format!("[PREVIOUS CONVERSATION HISTORY]\n{history}\n"));
        }
    }

    if let Some(last) = messages.last() {
        if last.role != "system" {
            let text = last.content.as_text();
            let prompt_text = if text.trim().is_empty() && has_media_attachments(last) {
                "Please analyze the attached image."
            } else {
                text.trim()
            };
            prompt.push_str(&format!("[USER]\n{}\n\n", prompt_text));
        }
    }

    prompt.trim().to_string()
}

fn has_media_attachments(msg: &ChatMessage) -> bool {
    let crate::domain::openai::MessageContent::Parts(parts) = &msg.content else {
        return false;
    };
    parts.iter().any(|p| {
        p.image_url.is_some() || p.file.is_some() || p.url.is_some() || p.data.is_some()
    })
}

fn extract_system_prompt(messages: &[ChatMessage], has_tools: bool) -> Option<String> {
    for msg in messages {
        if msg.role != "system" {
            continue;
        }
        let text = msg.content.as_text();
        if text.is_empty() {
            continue;
        }
        let full = if has_tools {
            format!("{}\n\n{}", text.trim(), TOOL_USE_INSTRUCTIONS)
        } else {
            text.trim().to_string()
        };
        return Some(full);
    }
    if has_tools {
        Some(TOOL_USE_INSTRUCTIONS.to_string())
    } else {
        None
    }
}

fn build_conversation_history(messages: &[ChatMessage]) -> String {
    let mut history = String::new();
    for msg in &messages[..messages.len() - 1] {
        if msg.role == "system" {
            continue;
        }
        let text = msg.content.as_text();
        let role = msg.role.to_uppercase();
        if let Some(tool_calls) = &msg.tool_calls {
            let mut tc_blocks = Vec::new();
            for tc in tool_calls {
                tc_blocks.push(format!(
                    "<tool_call>{{\"name\": \"{}\", \"arguments\": {}}}</tool_call>",
                    tc.function.name, tc.function.arguments
                ));
            }
            let calls_str = tc_blocks.join("\n");
            if text.trim().is_empty() {
                history.push_str(&format!("{role}: {calls_str}\n"));
            } else {
                history.push_str(&format!("{role}: {}\n{calls_str}\n", text.trim()));
            }
        } else if !text.trim().is_empty() {
            history.push_str(&format!("{role}: {}\n", text.trim()));
        }
    }
    history
}

fn build_continuing_prompt(messages: &[ChatMessage], tools: Option<&[Value]>) -> String {
    let mut last_ast_idx = None;
    for (i, msg) in messages.iter().enumerate().rev() {
        if msg.role == "assistant" {
            last_ast_idx = Some(i);
            break;
        }
    }

    let trailing: &[ChatMessage] = match last_ast_idx {
        Some(idx) if idx + 1 < messages.len() => &messages[idx + 1..],
        _ => match messages.last() {
            Some(last) => std::slice::from_ref(last),
            None => &[],
        },
    };

    let mut user_parts = Vec::new();
    for msg in trailing {
        if msg.role != "user" && msg.role != "tool" {
            continue;
        }
        let text = msg.content.as_text();
        if text.is_empty() {
            continue;
        }
        if msg.role == "tool" {
            user_parts.push(format!("[TOOL OUTPUT]\n{}", text.trim()));
        } else {
            user_parts.push(text);
        }
    }

    let mut result = if user_parts.is_empty() {
        "Continue.".to_string()
    } else {
        user_parts.join("\n\n")
    };

    let has_tools = tools.map(|t| !t.is_empty()).unwrap_or(false);
    if has_tools {
        result.push_str("\n\n");
        result.push_str(TOOL_USE_INSTRUCTIONS);
    }

    result
}
