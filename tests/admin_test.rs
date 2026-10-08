//! Coverage for the programmatic admin API (`/api/admin/*`).
//!
//! Exercises authentication, the token-pool CRUD surface, status control,
//! usage reset and the guarantee that raw credentials never leak.

use deeperseeker::api::build_router;
use deeperseeker::api::live_log::LiveLog;
use deeperseeker::api::state::AppState;
use deeperseeker::config::AppConfig;
use deeperseeker::infra::db::{add_token, get_tokens, init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use deeperseeker::infra::usage_db::record_usage_attributed;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::Mutex;
use tokio_rusqlite::Connection;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const POW_CHALLENGE_BODY: &str = r#"{"code":0,"data":{"biz_data":{"challenge":{"algorithm":"DeepSeekHashV1","challenge":"56792d7dd642aa1c27191ba9710df5ceb3504cd3a55b225b13f6dad53356f560","salt":"4e8565ce57578f4efb4d","difficulty":144000,"expire_at":1791027488061,"signature":"sig"}}}}"#;

const MASTER_KEY: &str = "dseeker";

fn test_config(db_path: &str) -> AppConfig {
    AppConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        api_key: MASTER_KEY.to_string(),
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
        upstream_timeout_secs: 30,
        upstream_connect_timeout_secs: 5,
        session_retention_days: 0,
        usage_retention_days: 0,
    }
}

fn temp_db_path(tag: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "ds_admin_{}_{}_{:?}.db",
        tag,
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_file(&path);
    path.to_str().expect("temp db path").to_string()
}

/// Spawn the real router pointed at `upstream`. Returns base URL and DB handle.
async fn spawn_server(tag: &str, upstream: &str) -> (String, Connection) {
    let config = test_config(&temp_db_path(tag));
    let db = open_db(&config.db_path).await.expect("open db");
    init_db(&db).await.expect("init db");

    let pow_solver = Arc::new(PowSolver::new(&config.wasm_path).expect("pow solver"));
    let pattern = deeperseeker::infra::assets::resolve_templates_pattern();
    let tera = Arc::new(Tera::new(&pattern).expect("tera templates"));

    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        client: DeepSeekClient::with_base_url(upstream, 30, 5),
        pow_solver,
        in_flight: Arc::new(Mutex::new(HashMap::new())),
        client_gates: Arc::new(Mutex::new(HashMap::new())),
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

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn json_body(resp: reqwest::Response) -> serde_json::Value {
    let text = resp.text().await.expect("body");
    serde_json::from_str(&text).unwrap_or(serde_json::json!({ "raw": text }))
}

#[tokio::test]
async fn admin_endpoints_require_master_key() {
    let (base, _db) = spawn_server("auth", "http://127.0.0.1:1").await;

    let missing = client()
        .get(format!("{base}/api/admin/tokens"))
        .send()
        .await
        .expect("request");
    assert_eq!(missing.status(), 401, "no bearer must be rejected");

    let wrong = client()
        .get(format!("{base}/api/admin/tokens"))
        .header("Authorization", "Bearer dsk-not-the-master")
        .send()
        .await
        .expect("request");
    assert_eq!(wrong.status(), 401, "client keys must not administer");
}

#[tokio::test]
async fn pool_and_token_listing_expose_masked_credentials() {
    let (base, db) = spawn_server("list", "http://127.0.0.1:1").await;
    add_token(
        &db,
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.secret",
        Some("primary"),
    )
    .await
    .expect("add token");

    let pool = client()
        .get(format!("{base}/api/admin/pool"))
        .header("Authorization", format!("Bearer {MASTER_KEY}"))
        .send()
        .await
        .expect("request");
    assert_eq!(pool.status(), 200);
    let pool = json_body(pool).await;
    assert_eq!(pool["pool"]["total"], 1);
    assert_eq!(pool["pool"]["active"], 1);

    let list = client()
        .get(format!("{base}/api/admin/tokens"))
        .header("Authorization", format!("Bearer {MASTER_KEY}"))
        .send()
        .await
        .expect("request");
    assert_eq!(list.status(), 200);
    let list = json_body(list).await;
    assert_eq!(list["tokens"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(list["tokens"][0]["alias"], "primary");
    let masked = list["tokens"][0]["masked_token"]
        .as_str()
        .unwrap_or_default();
    assert!(masked.contains('…'), "masked form expected, got {masked}");
    assert!(
        !list.to_string().contains("secret"),
        "raw credential must never leak: {list}"
    );
}

#[tokio::test]
async fn add_get_update_and_delete_round_trip() {
    let (base, _db) = spawn_server("crud", "http://127.0.0.1:1").await;
    let auth = format!("Bearer {MASTER_KEY}");

    let add = client()
        .post(format!("{base}/api/admin/tokens"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({
            "auth_token": "  \"eyJ-test-credential-value-1234567890\"  ",
            "alias": "team-a"
        }))
        .send()
        .await
        .expect("request");
    assert_eq!(add.status(), 200, "add must succeed");
    let add = json_body(add).await;
    let id = add["token"]["id"].as_i64().expect("new token id");

    let got = client()
        .get(format!("{base}/api/admin/tokens/{id}"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request");
    assert_eq!(got.status(), 200);

    // Alias-only update needs no upstream verification.
    let upd = client()
        .patch(format!("{base}/api/admin/tokens/{id}"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "alias": "team-b" }))
        .send()
        .await
        .expect("request");
    assert_eq!(upd.status(), 200, "alias update must succeed");

    let got = json_body(
        client()
            .get(format!("{base}/api/admin/tokens/{id}"))
            .header("Authorization", &auth)
            .send()
            .await
            .expect("request"),
    )
    .await;
    assert_eq!(got["token"]["alias"], "team-b");

    let del = client()
        .delete(format!("{base}/api/admin/tokens/{id}"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request");
    assert_eq!(del.status(), 200, "delete must succeed");

    let missing = client()
        .get(format!("{base}/api/admin/tokens/{id}"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request");
    assert_eq!(missing.status(), 404, "deleted token must be gone");
}

#[tokio::test]
async fn status_and_reset_control_token_lifecycle() {
    let (base, db) = spawn_server("status", "http://127.0.0.1:1").await;
    let auth = format!("Bearer {MASTER_KEY}");
    add_token(&db, "eyJ-lifecycle-token-1234567890abcdef", None)
        .await
        .expect("add token");
    let id = get_tokens(&db).await.expect("tokens")[0].id;

    record_usage_attributed(&db, "v4.1flash", 100, 50, 0, Some(id), None)
        .await
        .expect("record usage");

    let reset = client()
        .post(format!("{base}/api/admin/tokens/{id}/reset"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request");
    assert_eq!(reset.status(), 200);
    let reset = json_body(reset).await;
    assert_eq!(reset["usage_rows_deleted"], 1, "usage row must be pruned");

    let expired = client()
        .post(format!("{base}/api/admin/tokens/{id}/status"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "status": "expired" }))
        .send()
        .await
        .expect("request");
    assert_eq!(expired.status(), 200);
    assert_eq!(json_body(expired).await["token_status"], "EXPIRED");

    let active = client()
        .post(format!("{base}/api/admin/tokens/{id}/status"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "status": "ACTIVE" }))
        .send()
        .await
        .expect("request");
    assert_eq!(active.status(), 200);
    assert_eq!(json_body(active).await["token_status"], "ACTIVE");

    let bad = client()
        .post(format!("{base}/api/admin/tokens/{id}/status"))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "status": "banana" }))
        .send()
        .await
        .expect("request");
    assert_eq!(bad.status(), 400, "unknown status must be rejected");
}

#[tokio::test]
async fn verify_probes_upstream_and_updates_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v0/chat/create_pow_challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_string(POW_CHALLENGE_BODY))
        .mount(&server)
        .await;

    let (base, db) = spawn_server("verify", &server.uri()).await;
    let auth = format!("Bearer {MASTER_KEY}");
    add_token(&db, "eyJ-verify-token-1234567890abcdefgh", None)
        .await
        .expect("add token");
    let id = get_tokens(&db).await.expect("tokens")[0].id;

    let resp = client()
        .post(format!("{base}/api/admin/tokens/{id}/verify"))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(
        body["token_status"], "ACTIVE",
        "valid PoW must verify: {body}"
    );
}
