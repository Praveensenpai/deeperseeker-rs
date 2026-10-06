use deeperseeker::api::build_router;
use deeperseeker::api::live_log::{LiveLog, LogFinish, LogSeed};
use deeperseeker::api::state::AppState;
use deeperseeker::config::AppConfig;
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::Mutex;

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
    }
}

fn auth_cookie(user: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{user}:{secret}").as_bytes());
    format!("session={}", hex::encode(hasher.finalize()))
}

async fn spawn_server() -> (String, Arc<LiveLog>) {
    let db_path = std::env::temp_dir().join(format!(
        "ds_live_log_{}_{:?}.db",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&db_path);

    let config = test_config(db_path.to_str().expect("temp db path"));
    let db = open_db(&config.db_path).await.expect("open db");
    init_db(&db).await.expect("init db");

    let pow_solver = Arc::new(PowSolver::new(&config.wasm_path).expect("pow solver"));
    let pattern = deeperseeker::infra::assets::resolve_templates_pattern();
    let tera = Arc::new(Tera::new(&pattern).expect("tera templates"));
    let live_log = Arc::new(LiveLog::new());

    let state = AppState {
        config: Arc::new(config),
        db,
        client: DeepSeekClient::new(),
        pow_solver,
        in_flight: Arc::new(Mutex::new(HashMap::new())),
        tera,
        live_log: Arc::clone(&live_log),
    };

    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    (format!("http://{addr}"), live_log)
}

#[tokio::test]
async fn live_log_endpoints_require_auth_and_serve_entries() {
    let (base, live_log) = spawn_server().await;

    let handle = live_log.begin(LogSeed {
        model: "v4.1flash".to_string(),
        token_id: Some(7),
        session_id: "sess-smoke".to_string(),
        stream: true,
        input: "[user] hello".to_string(),
    });
    handle.append_content("world");
    handle.finish(LogFinish {
        prompt_tokens: 3,
        completion_tokens: 5,
        cached_tokens: 0,
        finish_reason: "stop".to_string(),
        tool_calls: 0,
    });

    let client = reqwest::Client::new();
    let cookie = auth_cookie("admin", "test-secret");

    let unauth = client
        .get(format!("{base}/api/logs"))
        .send()
        .await
        .expect("unauth request");
    assert_eq!(
        unauth.status(),
        401,
        "unauthenticated /api/logs must be 401"
    );

    let auth = client
        .get(format!("{base}/api/logs"))
        .header("Cookie", &cookie)
        .send()
        .await
        .expect("auth request");
    assert_eq!(auth.status(), 200);
    let body = auth.text().await.expect("body");
    assert!(body.contains("v4.1flash"), "missing model in {body}");
    assert!(body.contains("world"), "missing output in {body}");
    assert!(
        body.contains("\"status\":\"ok\""),
        "missing ok status in {body}"
    );
    assert!(body.contains("sess-smoke"), "missing session id in {body}");

    let unauth_stream = client
        .get(format!("{base}/api/logs/stream"))
        .send()
        .await
        .expect("unauth stream");
    assert_eq!(unauth_stream.status(), 401);

    let mut stream = client
        .get(format!("{base}/api/logs/stream"))
        .header("Cookie", &cookie)
        .send()
        .await
        .expect("auth stream");
    assert_eq!(stream.status(), 200);
    let chunk = stream.chunk().await.expect("chunk read").expect("snapshot");
    let text = String::from_utf8_lossy(&chunk);
    assert!(
        text.contains("event: snapshot"),
        "bad snapshot frame: {text}"
    );
    assert!(text.contains("v4.1flash"), "snapshot missing model: {text}");
}
