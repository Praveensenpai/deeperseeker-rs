use anyhow::{Context, Result};
use clap::Parser;
use deeperseeker::api::build_router;
use deeperseeker::api::state::AppState;
use deeperseeker::cli::diagnostic::run_diagnostics;
use deeperseeker::cli::service::{install_user_service, service_status, uninstall_user_service};
use deeperseeker::cli::token_cmd::{add_token, list_tokens, remove_token, test_tokens};
use deeperseeker::cli::usage_cmd::display_usage;
use deeperseeker::cli::{
    Cli, Commands, ServeArgs, ServiceArgs, ServiceSubcommands, TokenArgs, TokenSubcommands,
};
use deeperseeker::config::AppConfig;
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::pow::PowSolver;
use deeperseeker::tui::run_status;
use std::collections::HashMap;
use std::sync::Arc;
use tera::Tera;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => run_server(ServeArgs::default()).await,
        Some(Commands::Serve(args)) => run_server(args).await,
        Some(Commands::Status(args)) => run_status(&args.url, &args.db, args.plain).await,
        Some(Commands::Usage(args)) => {
            display_usage(&args.db, args.raw, args.days, args.json).await
        }
        Some(Commands::Token(args)) => handle_token(args).await,
        Some(Commands::Test(args)) => run_diagnostics(&args.db, &args.wasm, Some(&args.url)).await,
        Some(Commands::Service(args)) => handle_service(args),
    }
}

async fn handle_token(args: TokenArgs) -> Result<()> {
    match args.subcommand {
        TokenSubcommands::List { db } => list_tokens(&db).await,
        TokenSubcommands::Add { token, alias, db } => {
            add_token(&token, alias.as_deref(), &db).await
        }
        TokenSubcommands::Remove { id, db } => remove_token(id, &db).await,
        TokenSubcommands::Test { id, db } => test_tokens(id, &db).await,
    }
}

fn handle_service(args: ServiceArgs) -> Result<()> {
    match args.subcommand {
        ServiceSubcommands::Install { bin } => install_user_service(bin.as_deref()),
        ServiceSubcommands::Uninstall => uninstall_user_service(),
        ServiceSubcommands::Status => service_status(),
    }
}

async fn run_server(args: ServeArgs) -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let mut config = AppConfig::from_env();
    if let Some(h) = args.host {
        config.host = h;
    }
    if let Some(p) = args.port {
        config.port = p;
    }
    if let Some(d) = args.db {
        config.db_path = d;
    }
    if let Some(w) = args.wasm {
        config.wasm_path = w;
    }

    let config = Arc::new(config);
    let db = open_db(&config.db_path).await?;
    init_db(&db).await?;

    let pow_solver = Arc::new(
        PowSolver::new(&config.wasm_path).context("Failed initializing WASM PoW solver engine")?,
    );

    let client = DeepSeekClient::new();
    let tera = Arc::new(
        Tera::new("templates/**/*").context("Failed compiling HTML templates from templates/")?,
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
