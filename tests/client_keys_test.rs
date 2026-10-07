use deeperseeker::api::build_router;
use deeperseeker::api::live_log::LiveLog;
use deeperseeker::api::state::AppState;
use deeperseeker::config::AppConfig;
use deeperseeker::infra::client_keys::{
    add_client_key, delete_client_key, find_active_by_plaintext, list_client_keys, set_revoked,
    window_usage_tokens,
};
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use deeperseeker::infra::usage_db::record_usage_attributed;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::Mutex;
use tokio_rusqlite::Connection;

fn test_config(db_path: &str) -> AppConfig {
    AppConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        api_key: "dseeker".to_string(),
        admin_user: "admin".to_string(),
        admin_pass: "admin".to_string(),
        db_path: db_path.to_string(),
        wasm_path: deeperseeker::infra::assets::resolve_wasm_path("wasm/deepseek_pow_solver.wasm"),
        session_secret: "test-secret".to_string(),
        token_concurrency: 8,
        cookie_cooldown: 20,
        request_gap: 0.0,
        request_gap_jitter: 0.0,
        human_pause_chance: 0.0,
        human_pause_max: 0.0,
        suspend_probe_interval_secs: 3600,
        upstream_timeout_secs: 300,
        upstream_connect_timeout_secs: 15,
        session_retention_days: 0,
        usage_retention_days: 0,
    }
}

fn temp_db_path(tag: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "ds_client_keys_{}_{}_{:?}.db",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    path.to_str().expect("temp db path").to_string()
}

async fn open_test_db(tag: &str) -> Connection {
    let db = open_db(&temp_db_path(tag)).await.expect("open db");
    init_db(&db).await.expect("init db");
    db
}

async fn spawn_server(tag: &str) -> (String, Connection) {
    let db_path = temp_db_path(tag);
    let config = test_config(&db_path);
    let db = open_db(&config.db_path).await.expect("open db");
    init_db(&db).await.expect("init db");

    let pow_solver = Arc::new(PowSolver::new(&config.wasm_path).expect("pow solver"));
    let pattern = deeperseeker::infra::assets::resolve_templates_pattern();
    let tera = Arc::new(Tera::new(&pattern).expect("tera templates"));

    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        client: DeepSeekClient::new(),
        pow_solver,
        in_flight: Arc::new(Mutex::new(HashMap::new())),
        tera,
        live_log: Arc::new(LiveLog::new()),
        metrics: Arc::new(deeperseeker::infra::metrics::Metrics::new()),
        tokenizer: deeperseeker::infra::tokenizer::Tokenizer::load(),
    };

    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    (format!("http://{addr}"), db)
}

#[tokio::test]
async fn store_creates_looks_up_revokes_and_deletes_keys() {
    let db = open_test_db("store").await;

    let (id, plaintext) = add_client_key(&db, "team-alpha", 1_000, 86_400)
        .await
        .expect("add key");
    assert!(plaintext.starts_with("dsk-"), "bad prefix: {plaintext}");
    assert_eq!(id, 1);

    let found = find_active_by_plaintext(&db, &plaintext)
        .await
        .expect("lookup")
        .expect("key should resolve");
    assert_eq!(found.id, id);
    assert_eq!(found.name, "team-alpha");
    assert_eq!(found.quota_tokens, 1_000);
    assert_eq!(found.window_secs, 86_400);
    assert_eq!(found.key_prefix.len(), 12);
    assert!(plaintext.starts_with(&found.key_prefix));
    assert!(!found.revoked);

    assert!(
        find_active_by_plaintext(&db, "dsk-does-not-exist")
            .await
            .expect("lookup miss")
            .is_none(),
        "unknown plaintext must not resolve"
    );

    record_usage_attributed(&db, "v4.1flash", 120, 80, 0, None, Some(id))
        .await
        .expect("record usage");
    let used = window_usage_tokens(&db, id, 86_400)
        .await
        .expect("window usage");
    assert_eq!(used, 200, "expected 200 tokens attributed to key");

    set_revoked(&db, id, true).await.expect("revoke");
    assert!(
        find_active_by_plaintext(&db, &plaintext)
            .await
            .expect("lookup after revoke")
            .is_none(),
        "revoked key must not authenticate"
    );

    let all = list_client_keys(&db).await.expect("list");
    assert_eq!(all.len(), 1);
    assert!(all[0].revoked);

    delete_client_key(&db, id).await.expect("delete");
    assert!(list_client_keys(&db)
        .await
        .expect("list after delete")
        .is_empty());
}

#[tokio::test]
async fn unknown_client_key_is_rejected_with_401() {
    let (base, _db) = spawn_server("unknown").await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("Authorization", "Bearer dsk-not-a-real-key")
        .json(&serde_json::json!({
            "model": "v4.1flash",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 401, "unknown key must be rejected");
}

#[tokio::test]
async fn master_key_still_authenticates() {
    let (base, _db) = spawn_server("master").await;

    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("Authorization", "Bearer dseeker")
        .json(&serde_json::json!({
            "model": "v4.1flash",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request");

    assert_ne!(resp.status(), 401, "master key must authenticate");
    assert_ne!(resp.status(), 429, "master key has no quota");
}

#[tokio::test]
async fn exhausted_quota_returns_429_with_retry_after() {
    let (base, db) = spawn_server("quota").await;

    let (id, plaintext) = add_client_key(&db, "team-quota", 100, 86_400)
        .await
        .expect("add key");
    record_usage_attributed(&db, "v4.1flash", 150, 50, 0, None, Some(id))
        .await
        .expect("record usage");

    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("Authorization", format!("Bearer {plaintext}"))
        .json(&serde_json::json!({
            "model": "v4.1flash",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status(), 429, "exhausted key must be rate limited");
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(retry_after, "86400", "Retry-After must reflect the window");

    let body = resp.text().await.expect("body");
    assert!(body.contains("quota_exceeded"), "missing code in: {body}");
    assert!(
        body.contains("used_tokens"),
        "missing used_tokens in: {body}"
    );
}
