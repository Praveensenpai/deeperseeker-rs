use deeperseeker::domain::openai::ChatMessage;
use deeperseeker::infra::prompt::build_prompt_for_turn;

#[test]
fn test_build_prompt_first_turn() {
    let messages = vec![
        ChatMessage::system("You are an expert coder."),
        ChatMessage::user("Write a hello world in Rust."),
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
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hello! How can I help?"),
        ChatMessage::user("What is 2+2?"),
    ];

    let prompt = build_prompt_for_turn(&messages, false);
    assert_eq!(prompt, "What is 2+2?");
}

#[test]
fn test_build_prompt_no_assistant_echo() {
    let messages = vec![
        ChatMessage::user("Write code"),
        ChatMessage::assistant("Here is the code..."),
    ];

    let prompt = build_prompt_for_turn(&messages, false);
    assert_eq!(prompt, "Continue.");
    assert!(!prompt.contains("Here is the code..."));
}

#[test]
fn test_build_prompt_with_tool_output() {
    let messages = vec![
        ChatMessage::user("List files"),
        ChatMessage::assistant("Running ls..."),
        ChatMessage::new("tool", "file1.txt\nfile2.txt"),
    ];

    let prompt = build_prompt_for_turn(&messages, false);
    assert!(prompt.contains("[TOOL OUTPUT]"));
    assert!(prompt.contains("file1.txt"));
}
