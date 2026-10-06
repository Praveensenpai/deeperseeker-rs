use deeperseeker::domain::openai::{ChatMessage, FunctionCall, ToolCall};
use deeperseeker::domain::session::{
    compute_next_signature, compute_signature, next_parent_id, Session,
};
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::session_db::{find_session, save_session};

#[test]
fn test_signature_computation() {
    let messages = vec![
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there!"),
        ChatMessage::user("How are you?"),
    ];

    let sig1 = compute_signature(&messages, "v4.1flash", "");
    assert!(!sig1.is_empty());
    assert_eq!(sig1.len(), 64);

    let next_parent = next_parent_id(0);
    assert_eq!(next_parent, 2);
    assert_eq!(next_parent_id(2), 4);
    assert_eq!(next_parent_id(4), 6);
}

#[test]
fn test_signature_with_tool_calls_matching() {
    let req_messages = vec![ChatMessage::user("can u see my projects")];
    let tool_calls = vec![ToolCall {
        index: Some(0),
        id: "call_abc123_0".to_string(),
        r#type: "function".to_string(),
        function: FunctionCall {
            name: "bash".to_string(),
            arguments: r#"{"command":"ls ~/Projects"}"#.to_string(),
        },
    }];

    // Signature computed when saving session in turn 1
    let sig_saved = compute_next_signature(
        &req_messages,
        "v4.1flash",
        "Let me check.",
        Some(&tool_calls),
    );

    // Next turn messages sent by OpenAI client in turn 2 (with trailing newlines & spaced JSON)
    let mut ast_msg = ChatMessage::assistant("Let me check.\n\n");
    ast_msg.tool_calls = Some(vec![ToolCall {
        index: None,
        id: "different_client_id".to_string(), // ID should not affect canonical matching
        r#type: "function".to_string(),
        function: FunctionCall {
            name: "bash ".to_string(),
            arguments: r#"{"command": "ls ~/Projects"}"#.to_string(),
        },
    }]);

    let turn2_messages = vec![
        ChatMessage::user("can u see my projects"),
        ast_msg,
        ChatMessage::new("tool", "project1\nproject2"),
    ];

    // Signature computed in try_resume_session in turn 2
    let sig_resumed = compute_signature(&turn2_messages, "v4.1flash", "");
    assert_eq!(
        sig_saved, sig_resumed,
        "Session signature MUST match between turn 1 and turn 2 despite whitespace differences!"
    );
}

#[tokio::test]
async fn test_db_session_persistence() {
    let db = open_db(":memory:").await.unwrap();
    init_db(&db).await.unwrap();

    let sig = "test-signature-12345";
    let sess = Session::new(1, "chat-session-abc".to_string(), 1, 0.0);
    save_session(&db, sig, &sess).await.unwrap();

    let found = find_session(&db, sig).await.unwrap();
    assert!(found.is_some());
    let unwrapped = found.unwrap();
    assert_eq!(unwrapped.token_id, 1);
    assert_eq!(unwrapped.session_id, "chat-session-abc");
    assert_eq!(unwrapped.parent_message_id, 1);
}
