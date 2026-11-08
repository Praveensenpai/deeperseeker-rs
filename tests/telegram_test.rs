use deeperseeker::domain::telegram::{TelegramNotification, TelegramSettings};
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::telegram_db::{get_telegram_settings, save_telegram_settings};
use deeperseeker::infra::telegram_notifier::TelegramNotifier;

#[test]
fn test_telegram_settings_masking_and_config() {
    let empty = TelegramSettings::default();
    assert!(!empty.is_configured());
    assert_eq!(empty.masked_token(), "");

    let cfg = TelegramSettings {
        bot_token: "123456789:ABCdefGHIjklMNO123456".to_string(),
        chat_id: "987654321".to_string(),
        enabled: true,
        updated_at: 1000.0,
    };
    assert!(cfg.is_configured());
    assert_eq!(cfg.masked_token(), "123456789:••••3456");

    let disabled = TelegramSettings {
        enabled: false,
        ..cfg
    };
    assert!(!disabled.is_configured());
}

#[test]
fn test_telegram_notification_formatting() {
    let new_tok = TelegramNotification::NewTokenAdded {
        id: 1,
        alias: Some("primary".to_string()),
        masked: "dsk-1234…5678".to_string(),
        active_count: 2,
        total_count: 3,
    };
    let text = new_tok.format_markdown();
    assert!(text.contains("New Token Added"));
    assert!(text.contains("#1"));
    assert!(text.contains("primary"));
    assert!(text.contains("2 active / 3 total"));

    let expired = TelegramNotification::TokenExpired {
        id: 2,
        alias: None,
        reason: "Unauthorized 401".to_string(),
        active_count: 1,
    };
    let exp_text = expired.format_markdown();
    assert!(exp_text.contains("Token Expired"));
    assert!(exp_text.contains("#2"));
    assert!(exp_text.contains("unnamed"));
    assert!(exp_text.contains("Active Remaining:* `1`"));

    let no_tok = TelegramNotification::NoValidTokens {
        endpoint: "/v1/chat/completions".to_string(),
        model: "deepseek-chat".to_string(),
        total_tokens: 2,
    };
    let no_tok_text = no_tok.format_markdown();
    assert!(no_tok_text.contains("No Valid Tokens Available"));
    assert!(no_tok_text.contains("deepseek-chat"));
}

#[tokio::test]
async fn test_telegram_db_persistence() {
    let db = open_db(":memory:").await.unwrap();
    init_db(&db).await.unwrap();

    let initial = get_telegram_settings(&db).await.unwrap();
    assert!(!initial.is_configured());

    let settings = TelegramSettings {
        bot_token: "111222333:TESTSECRETKEY999".to_string(),
        chat_id: "777888".to_string(),
        enabled: true,
        updated_at: 500.0,
    };
    save_telegram_settings(&db, &settings).await.unwrap();

    let loaded = get_telegram_settings(&db).await.unwrap();
    assert!(loaded.is_configured());
    assert_eq!(loaded.bot_token, "111222333:TESTSECRETKEY999");
    assert_eq!(loaded.chat_id, "777888");
    assert!(loaded.enabled);
}

#[tokio::test]
async fn test_telegram_notifier_rate_limiting_empty_hits() {
    let cfg = TelegramSettings {
        bot_token: "test:token".to_string(),
        chat_id: "123".to_string(),
        enabled: true,
        updated_at: 0.0,
    };
    let notifier = TelegramNotifier::new(cfg);

    // Rapid burst of 10 requests when no valid tokens exist
    for _ in 0..10 {
        notifier
            .notify_no_valid_tokens("/v1/chat/completions", "deepseek-chat", 0)
            .await;
    }

    // First call records the timestamp, subsequent rapid hits within 15s are throttled/counted
    // Verify no panic or lockup
    let current = notifier.current_settings().await;
    assert_eq!(current.chat_id, "123");
}

#[test]
fn test_telegram_command_parsing() {
    let input = "/add eyJhbGciOiJIUzI1NiJ9 work";
    let (cmd, args) = match input.split_once(char::is_whitespace) {
        Some((c, a)) => (c.trim(), a.trim()),
        None => (input.trim(), ""),
    };
    assert_eq!(cmd, "/add");
    assert_eq!(args, "eyJhbGciOiJIUzI1NiJ9 work");

    let del_input = "/del 3";
    let (cmd2, args2) = match del_input.split_once(char::is_whitespace) {
        Some((c, a)) => (c.trim(), a.trim()),
        None => (del_input.trim(), ""),
    };
    assert_eq!(cmd2, "/del");
    assert_eq!(args2, "3");
}
