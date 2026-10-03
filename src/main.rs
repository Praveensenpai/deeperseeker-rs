use deeperseeker::api::build_router;
use deeperseeker::api::state::AppState;
use deeperseeker::config::AppConfig;
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let config = Arc::new(AppConfig::from_env());
    let db = open_db(&config.db_path).await?;
    init_db(&db).await?;

    let pow_solver = Arc::new(
        PowSolver::new(&config.wasm_path)
            .context("Failed initializing WASM PoW solver engine")?,
    );

    let client = DeepSeekClient::new();
    let tera = Arc::new(
        Tera::new("templates/**/*")
            .context("Failed compiling HTML templates from templates/")?,
    );

    let in_flight = Arc::new(Mutex::new(HashMap::new()));
    let state = AppState {
        config: config.clone(),
        db,
        client,
        pow_solver,
        in_flight,
        tera,
    };

    let router = build_router(state);
    let bind_addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("Failed binding TCP listener to {bind_addr}"))?;

    info!("🚀 deeperseeker-rs listening on http://{bind_addr}");
    info!("OpenAI Base URL: http://{bind_addr}/v1");
    info!("Dashboard:       http://{bind_addr}/dashboard");

    axum::serve(listener, router)
        .await
        .context("Server exited unexpectedly")?;

    Ok(())
}
