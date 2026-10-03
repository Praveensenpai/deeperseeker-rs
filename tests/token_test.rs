use deeperseeker::domain::token::Token;
use deeperseeker::infra::db::{
    add_token, delete_token, get_tokens, init_db, mark_active, mark_limited, open_db, pick_token,
};
use std::collections::HashMap;

#[test]
fn test_token_masking() {
    let tok = Token {
        id: 1,
        alias: Some("test".to_string()),
        token: "pGfGi7QsGc44f7jmyBAFxApB1Y1C6UF4JLVEgkihGbQadC2MzNTEiay69cDslMUM".to_string(),
        status: "ACTIVE".to_string(),
        rate_limited_until: None,
        last_used: None,
    };
    assert_eq!(tok.masked_token(), "pGfGi7QsGc44…lMUM");
    assert!(tok.is_active());
    assert!(!tok.is_rate_limited());
}

#[tokio::test]
async fn test_db_token_lifecycle() {
    let db = open_db(":memory:").await.unwrap();
    init_db(&db).await.unwrap();

    add_token(&db, "test-token-1", Some("work")).await.unwrap();
    add_token(&db, "test-token-2", Some("personal")).await.unwrap();

    let tokens = get_tokens(&db).await.unwrap();
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].id, 1);
    assert_eq!(tokens[1].id, 2);

    mark_limited(&db, 1, 60).await.unwrap();
    let tokens_after_limit = get_tokens(&db).await.unwrap();
    assert_eq!(tokens_after_limit[0].status, "RATE_LIMITED");

    let in_flight = HashMap::new();
    let picked = pick_token(&db, &[], &in_flight, 8).await.unwrap();
    assert!(picked.is_some());
    assert_eq!(picked.unwrap().id, 2);

    mark_active(&db, 1).await.unwrap();
    let tokens_active = get_tokens(&db).await.unwrap();
    assert_eq!(tokens_active[0].status, "ACTIVE");

    delete_token(&db, 1).await.unwrap();
    let remaining = get_tokens(&db).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, 2);
}
