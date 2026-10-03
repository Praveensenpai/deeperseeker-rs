use deeperseeker::domain::openai::{ChatMessage, MessageContent};
use deeperseeker::domain::session::{compute_signature, next_parent_id, Session};
use deeperseeker::infra::db::{find_session, init_db, open_db, save_session};

#[test]
fn test_signature_computation() {
    let messages = vec![
        ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("Hello".to_string()),
            name: None,
        },
        ChatMessage {
            role: "assistant".to_string(),
            content: MessageContent::Text("Hi there!".to_string()),
            name: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: MessageContent::Text("How are you?".to_string()),
            name: None,
        },
    ];

    let sig1 = compute_signature(&messages, "v4.1flash", "");
    assert!(!sig1.is_empty());
    assert_eq!(sig1.len(), 64);

    let next_parent = next_parent_id(0);
    assert_eq!(next_parent, 2);
    assert_eq!(next_parent_id(2), 4);
    assert_eq!(next_parent_id(4), 6);
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
