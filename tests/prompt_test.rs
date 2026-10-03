use deeperseeker::domain::openai::{ChatMessage, MessageContent};
use deeperseeker::infra::prompt::build_prompt_for_turn;

#[test]
fn test_build_prompt_first_turn() {
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: MessageContent::Text("You are an expert coder.".to_string()),
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("Write a hello world in Rust.".to_string()),
            name: None,
        },
    ];

    let prompt = build_prompt_for_turn(&messages, true);
    assert!(prompt.contains("[SYSTEM PROMPT]"));
    assert!(prompt.contains("You are an expert coder."));
    assert!(prompt.contains("[USER]"));
    assert!(prompt.contains("Write a hello world in Rust."));
}

#[test]
fn test_build_prompt_continuing_turn() {
    let messages = vec![
        ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
            name: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: MessageContent::Text("Hello! How can I help?".to_string()),
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("What is 2+2?".to_string()),
            name: None,
        },
    ];

    let prompt = build_prompt_for_turn(&messages, false);
    assert_eq!(prompt, "What is 2+2?");
}
