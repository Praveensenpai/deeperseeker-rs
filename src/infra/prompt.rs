use crate::domain::openai::ChatMessage;

pub fn build_prompt_for_turn(messages: &[ChatMessage], is_first: bool) -> String {
    if is_first {
        build_first_turn_prompt(messages)
    } else {
        build_continuing_prompt(messages)
    }
}

fn build_first_turn_prompt(messages: &[ChatMessage]) -> String {
    let mut prompt = String::new();

    // Extract system prompt
    for msg in messages {
        if msg.role == "system" {
            let text = msg.content.as_text();
            if !text.is_empty() {
                prompt.push_str(&format!("[SYSTEM PROMPT]\n{}\n\n", text.trim()));
            }
        }
    }

    // Previous history if multi-turn
    if messages.len() > 1 {
        let mut history = String::new();
        for msg in &messages[..messages.len() - 1] {
            if msg.role == "system" {
                continue;
            }
            let text = msg.content.as_text();
            if !text.is_empty() {
                history.push_str(&format!("{}: {}\n", msg.role.to_uppercase(), text.trim()));
            }
        }
        if !history.is_empty() {
            prompt.push_str(&format!("[PREVIOUS CONVERSATION HISTORY]\n{history}\n"));
        }
    }

    // Latest user message
    if let Some(last) = messages.last() {
        if last.role != "system" {
            let text = last.content.as_text();
            prompt.push_str(&format!("[USER]\n{}\n\n", text.trim()));
        }
    }

    prompt.trim().to_string()
}

fn build_continuing_prompt(messages: &[ChatMessage]) -> String {
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
        if msg.role == "user" || msg.role == "tool" {
            let text = msg.content.as_text();
            if !text.is_empty() {
                if msg.role == "tool" {
                    user_parts.push(format!("[TOOL OUTPUT]\n{}", text.trim()));
                } else {
                    user_parts.push(text);
                }
            }
        }
    }

    if user_parts.is_empty() {
        return "Continue.".to_string();
    }

    user_parts.join("\n\n")
}
