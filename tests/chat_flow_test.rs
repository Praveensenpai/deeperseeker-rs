//! End-to-end coverage of the chat path against a mock DeepSeek upstream.
//!
//! These tests exercise the real router, session persistence, PoW solving,
//! SSE parsing and the retry/429 logic together. The upstream is a wiremock
//! server and `DeepSeekClient` is pointed at it via `with_base_url`.

use deeperseeker::api::build_router;
use deeperseeker::api::live_log::LiveLog;
use deeperseeker::api::state::AppState;
use deeperseeker::config::AppConfig;
use deeperseeker::infra::db::{add_token, init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::sync::Mutex;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SESSION_CREATE_BODY: &str =
    r#"{"code":0,"data":{"biz_data":{"chat_session":{"id":"mock-session-1"}}}}"#;

// The PoW WASM only accepts real-format challenges. This is the same
// benchmark vector `cli/diagnostic.rs` and `tui.rs` use to prove the solver
// works, so it is guaranteed to produce a valid answer.
const POW_CHALLENGE_BODY: &str = r#"{"code":0,"data":{"biz_data":{"challenge":{"algorithm":"DeepSeekHashV1","challenge":"56792d7dd642aa1c27191ba9710df5ceb3504cd3a55b225b13f6dad53356f560","salt":"4e8565ce57578f4efb4d","difficulty":144000,"expire_at":1791027488061,"signature":"sig"}}}}"#;

/// One RESPONSE fragment followed by the terminator, as DeepSeek emits it.
const SSE_BODY: &str = "data: {\"p\":\"response/fragments\",\"v\":[{\"type\":\"RESPONSE\",\"content\":\"Hello from mock\"}]}\n\ndata: [DONE]\n\n";

const REPLY: &str = "Hello from mock";

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
        upstream_timeout_secs: 30,
        upstream_connect_timeout_secs: 5,
        session_retention_days: 0,
        usage_retention_days: 0,
    }
}

fn sse_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(SSE_BODY, "text/event-stream")
}

fn non_sse_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        r#"{"code":500,"msg":"too many requests"}"#,
        "application/json",
    )
}

/// A well-formed SSE stream that carries no content fragment.
const EMPTY_SSE_BODY: &str = "data: [DONE]\n\n";

fn empty_sse_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(EMPTY_SSE_BODY, "text/event-stream")
}

/// A transport-level context-overflow rejection: HTTP 200 with a JSON error
/// body, which `send_completion_request` surfaces as an `Err`. The proxy must
/// classify it as a client error, not a rate limit.
fn context_overflow_non_sse_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(
        r#"{"code":40004,"msg":"maximum context length exceeded","data":null}"#,
        "application/json",
    )
}

/// The same overflow signalled inside an SSE stream, as the streaming path
/// sees it (non-zero `code` becomes an upstream error).
const CONTEXT_OVERFLOW_SSE: &str =
    "data: {\"code\":40004,\"msg\":\"maximum context length exceeded\",\"data\":null}\n\n";

fn context_overflow_sse_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(CONTEXT_OVERFLOW_SSE, "text/event-stream")
}

/// Mount the three upstream endpoints. The completion responder is caller
/// supplied so each test can control stream vs error behaviour.
async fn mount_upstream(server: &MockServer, completion: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/api/v0/chat_session/create"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SESSION_CREATE_BODY))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v0/chat/create_pow_challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_string(POW_CHALLENGE_BODY))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v0/chat/completion"))
        .respond_with(completion)
        .mount(server)
        .await;
}

fn unique_db(label: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "ds_flow_{}_{}_{label}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let _ = std::fs::remove_file(&path);
    path.to_string_lossy().to_string()
}

/// Start the real router with `token_count` active tokens, pointed at the
/// mock upstream. Returns the base URL.
async fn spawn_app(upstream: &str, token_count: usize, label: &str) -> String {
    let config = test_config(&unique_db(label));
    let db = open_db(&config.db_path).await.expect("open db");
    init_db(&db).await.expect("init db");

    for i in 0..token_count {
        add_token(&db, &format!("mock-token-{i}"), None)
            .await
            .expect("add token");
    }

    let pow_solver = Arc::new(PowSolver::new(&config.wasm_path).expect("pow solver"));
    let pattern = deeperseeker::infra::assets::resolve_templates_pattern();
    let tera = Arc::new(Tera::new(&pattern).expect("tera templates"));

    let state = AppState {
        config: Arc::new(config),
        db,
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

    format!("http://{addr}")
}

fn chat_body(stream: bool, messages: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "model": "v4.1flash",
        "stream": stream,
        "messages": messages,
    })
}

async fn post_chat(base: &str, body: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .header("Authorization", "Bearer dseeker")
        .json(&body)
        .send()
        .await
        .expect("chat request")
}

#[tokio::test]
async fn unary_completion_round_trip() {
    let server = MockServer::start().await;
    mount_upstream(&server, sse_response()).await;
    let base = spawn_app(&server.uri(), 1, "unary").await;

    let resp = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([{"role": "user", "content": "hi"}]),
        ),
    )
    .await;

    let status = resp.status();
    let text = resp.text().await.expect("body");
    assert_eq!(status, 200, "unary chat should succeed, body: {text}");
    let body: serde_json::Value = serde_json::from_str(&text).expect("json body");
    assert_eq!(
        body["choices"][0]["message"]["content"], REPLY,
        "content must round-trip from the upstream SSE fragment"
    );
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert!(
        body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) >= 1,
        "usage must be recorded: {body}"
    );
}

#[tokio::test]
async fn stream_completion_emits_openai_sse() {
    let server = MockServer::start().await;
    mount_upstream(&server, sse_response()).await;
    let base = spawn_app(&server.uri(), 1, "stream").await;

    let resp = post_chat(
        &base,
        chat_body(true, serde_json::json!([{"role": "user", "content": "hi"}])),
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/event-stream")
    );
    let body = resp.text().await.expect("stream body");
    assert!(body.contains(REPLY), "stream must carry the reply: {body}");
    assert!(
        body.contains("data: [DONE]"),
        "stream must terminate with [DONE]: {body}"
    );
}

#[tokio::test]
async fn session_is_reused_across_turns() {
    let server = MockServer::start().await;

    // The whole point: a second turn must resume, not create a new session.
    Mock::given(method("POST"))
        .and(path("/api/v0/chat_session/create"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SESSION_CREATE_BODY))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v0/chat/create_pow_challenge"))
        .respond_with(ResponseTemplate::new(200).set_body_string(POW_CHALLENGE_BODY))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/v0/chat/completion"))
        .respond_with(sse_response())
        .mount(&server)
        .await;

    let base = spawn_app(&server.uri(), 1, "chain").await;

    let first = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([{"role": "user", "content": "hi"}]),
        ),
    )
    .await;
    assert_eq!(first.status(), 200);

    let second = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": REPLY},
                {"role": "user", "content": "again"},
            ]),
        ),
    )
    .await;
    assert_eq!(second.status(), 200);

    // `.expect(1)` plus verify() proves the second turn resumed the session.
    server.verify().await;
}

#[tokio::test]
async fn empty_stream_does_not_retire_tokens() {
    let server = MockServer::start().await;
    // A well-formed SSE stream that never emits a content fragment yields a
    // 0-token completion — the transient hiccup this fix targets.
    mount_upstream(&server, empty_sse_response()).await;
    let base = spawn_app(&server.uri(), 1, "empty_stream").await;

    let resp = post_chat(
        &base,
        chat_body(true, serde_json::json!([{"role": "user", "content": "hi"}])),
    )
    .await;
    // Drain the stream so its retry/cleanup path runs to completion.
    let _ = resp.text().await;

    // An empty upstream response is a transient hiccup, not a dead credential:
    // the token must still be ACTIVE. The old code marked it RATE_LIMITED here,
    // so a few blank responses could sideline the whole pool.
    let pool: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/admin/pool"))
        .header("Authorization", "Bearer dseeker")
        .send()
        .await
        .expect("pool request")
        .json()
        .await
        .expect("pool json");
    assert_eq!(
        pool["pool"]["active"].as_u64().unwrap_or(0),
        1,
        "a token must remain ACTIVE after an empty stream: {pool}"
    );
    assert_eq!(
        pool["pool"]["rate_limited"].as_u64().unwrap_or(0),
        0,
        "an empty stream must not rate-limit a token: {pool}"
    );
}

#[tokio::test]
async fn large_request_bodies_are_accepted() {
    // Axum's 2 MB default body limit would reject the long contexts this
    // proxy advertises. A ~3 MB message must not be turned into a 413.
    let server = MockServer::start().await;
    mount_upstream(&server, sse_response()).await;
    let base = spawn_app(&server.uri(), 1, "large_body").await;

    let filler = "word ".repeat(700_000); // ~3.5 MB of text
    let resp = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([{"role": "user", "content": filler}]),
        ),
    )
    .await;

    assert_ne!(
        resp.status(),
        reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        "a multi-megabyte prompt must not be rejected by the body limit"
    );
}

#[tokio::test]
async fn exhausted_tokens_return_429_with_retry_after() {
    let server = MockServer::start().await;
    // Every completion fails the SSE check, so each attempt rate-limits a
    // token. With five tokens the retry loop exhausts and hits the 429 path.
    mount_upstream(&server, non_sse_response()).await;
    let base = spawn_app(&server.uri(), 5, "exhausted").await;

    let resp = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([{"role": "user", "content": "hi"}]),
        ),
    )
    .await;

    assert_eq!(
        resp.status(),
        429,
        "all tokens rate limited must surface as 429"
    );
    assert!(
        resp.headers().get("retry-after").is_some(),
        "429 must carry a Retry-After header"
    );
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["error"]["code"], "rate_limit_exceeded");
    assert!(
        body["error"]["retry_after"].as_u64().unwrap_or(0) > 0,
        "body must advertise the retry delay: {body}"
    );
}

#[tokio::test]
async fn context_overflow_is_a_client_error_not_a_rate_limit() {
    let server = MockServer::start().await;
    // Every token sees the same too-long rejection. The proxy must not rotate
    // the pool or cool anything down; it must return 400 context_length_exceeded
    // so an auto-compacting client (OpenCode) can recover.
    mount_upstream(&server, context_overflow_non_sse_response()).await;
    let base = spawn_app(&server.uri(), 3, "ctx_overflow").await;

    let resp = post_chat(
        &base,
        chat_body(
            false,
            serde_json::json!([{"role": "user", "content": "hi"}]),
        ),
    )
    .await;

    assert_eq!(
        resp.status(),
        400,
        "a context overflow must surface as 400, not 429"
    );
    let body: serde_json::Value = resp.json().await.expect("json body");
    assert_eq!(body["error"]["code"], "context_length_exceeded");
    assert_eq!(body["error"]["type"], "invalid_request_error");

    // The pool must be untouched: no token cooled down, none rotated out.
    let pool: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/admin/pool"))
        .header("Authorization", "Bearer dseeker")
        .send()
        .await
        .expect("pool request")
        .json()
        .await
        .expect("pool json");
    assert_eq!(
        pool["pool"]["rate_limited"].as_u64().unwrap_or(0),
        0,
        "a context overflow must not rate-limit any token: {pool}"
    );
    assert_eq!(
        pool["pool"]["active"].as_u64().unwrap_or(0),
        3,
        "all tokens must stay ACTIVE after a context overflow: {pool}"
    );
}

#[tokio::test]
async fn streaming_context_overflow_is_not_retried() {
    let server = MockServer::start().await;
    mount_upstream(&server, context_overflow_sse_response()).await;
    let base = spawn_app(&server.uri(), 3, "ctx_overflow_stream").await;

    let resp = post_chat(
        &base,
        chat_body(true, serde_json::json!([{"role": "user", "content": "hi"}])),
    )
    .await;

    // A streamed overflow must surface as a real HTTP error so the client's
    // API-error classifier sees it; an in-band 200 SSE frame is not parsed by
    // auto-compacting clients.
    assert_eq!(
        resp.status(),
        400,
        "a streaming context overflow must be a 400, not a 200 SSE frame"
    );
    let body: serde_json::Value = resp.json().await.expect("json error body");
    assert_eq!(body["error"]["code"], "context_length_exceeded");
    assert_eq!(body["error"]["type"], "invalid_request_error");

    // The session-create mock is only mounted once by `mount_upstream`, so a
    // retry would have attempted a fresh session. The pool check proves the
    // token was not rotated or cooled down.
    let pool: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/api/admin/pool"))
        .header("Authorization", "Bearer dseeker")
        .send()
        .await
        .expect("pool request")
        .json()
        .await
        .expect("pool json");
    assert_eq!(
        pool["pool"]["rate_limited"].as_u64().unwrap_or(0),
        0,
        "a streaming context overflow must not rate-limit a token: {pool}"
    );
    assert_eq!(
        pool["pool"]["active"].as_u64().unwrap_or(0),
        3,
        "all tokens must stay ACTIVE after a streaming context overflow: {pool}"
    );
}
