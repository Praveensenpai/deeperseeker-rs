use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub api_key: String,
    pub admin_user: String,
    pub admin_pass: String,
    pub db_path: String,
    pub wasm_path: String,
    pub session_secret: String,
    pub token_concurrency: usize,
    pub cookie_cooldown: u64,
    pub request_gap: f64,
    pub request_gap_jitter: f64,
    pub human_pause_chance: f64,
    pub human_pause_max: f64,
    pub suspend_probe_interval_secs: u64,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let host = env::var("DEEPSEEKER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("DEEPSEEKER_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(4000);
        let api_key = env::var("DEEPSEEKER_API_KEY").unwrap_or_else(|_| "dseeker".to_string());
        let admin_user = env::var("DEEPSEEKER_ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
        let admin_pass = env::var("DEEPSEEKER_ADMIN_PASS").unwrap_or_else(|_| "admin".to_string());
        let db_path = crate::infra::assets::resolve_db_path(None);
        let wasm_path = env::var("DEEPSEEKER_WASM_PATH")
            .map(|p| crate::infra::assets::resolve_wasm_path(&p))
            .unwrap_or_else(|_| {
                crate::infra::assets::resolve_wasm_path("wasm/deepseek_pow_solver.wasm")
            });
        let session_secret = env::var("DEEPSEEKER_SESSION_SECRET")
            .unwrap_or_else(|_| "deeperseeker-secret-session-key-32-chars!!".to_string());
        let token_concurrency = env::var("DEEPSEEKER_TOKEN_CONCURRENCY")
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or(8);
        let cookie_cooldown = env::var("DEEPSEEKER_COOKIE_COOLDOWN")
            .ok()
            .and_then(|c| c.parse().ok())
            .unwrap_or(20);
        let request_gap = env::var("DEEPSEEKER_REQUEST_GAP")
            .ok()
            .and_then(|g| g.parse().ok())
            .unwrap_or(5.0);
        let request_gap_jitter = env::var("DEEPSEEKER_REQUEST_GAP_JITTER")
            .ok()
            .and_then(|g| g.parse().ok())
            .unwrap_or(0.75);
        let human_pause_chance = env::var("DEEPSEEKER_HUMAN_PAUSE_CHANCE")
            .ok()
            .and_then(|g| g.parse().ok())
            .unwrap_or(0.12);
        let human_pause_max = env::var("DEEPSEEKER_HUMAN_PAUSE_MAX")
            .ok()
            .and_then(|g| g.parse().ok())
            .unwrap_or(6.0);
        let suspend_probe_interval_secs = env::var("DEEPSEEKER_SUSPEND_PROBE_INTERVAL_HOURS")
            .ok()
            .and_then(|h| h.parse::<u64>().ok())
            .map(|h| h * 3600)
            .unwrap_or(6 * 3600);

        Self {
            host,
            port,
            api_key,
            admin_user,
            admin_pass,
            db_path,
            wasm_path,
            session_secret,
            token_concurrency,
            cookie_cooldown,
            request_gap,
            request_gap_jitter,
            human_pause_chance,
            human_pause_max,
            suspend_probe_interval_secs,
        }
    }
}
