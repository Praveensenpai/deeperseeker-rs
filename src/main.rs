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
    UpdateArgs,
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
        Some(Commands::Status(args)) => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(args.db.as_deref());
            run_status(&args.url, &db_path, args.plain).await
        }
        Some(Commands::Usage(args)) => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(args.db.as_deref());
            display_usage(deeperseeker::cli::usage_cmd::UsageViewArgs {
                db_path,
                raw: args.raw,
                days: args.days,
                as_json: args.json,
                model: args.model,
                token: args.token,
            })
            .await
        }
        Some(Commands::Token(args)) => handle_token(args).await,
        Some(Commands::Test(args)) => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(args.db.as_deref());
            let wasm_path = deeperseeker::infra::assets::resolve_wasm_path(&args.wasm);
            run_diagnostics(&db_path, &wasm_path, Some(&args.url)).await
        }
        Some(Commands::Service(args)) => handle_service(args),
        Some(Commands::Update(args)) => handle_update(args).await,
    }
}

async fn handle_token(args: TokenArgs) -> Result<()> {
    match args.subcommand {
        TokenSubcommands::List { db } => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(db.as_deref());
            list_tokens(&db_path).await
        }
        TokenSubcommands::Add { token, alias, db } => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(db.as_deref());
            add_token(&token, alias.as_deref(), &db_path).await
        }
        TokenSubcommands::Remove { id, db } => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(db.as_deref());
            remove_token(id, &db_path).await
        }
        TokenSubcommands::Test { id, db } => {
            let db_path = deeperseeker::infra::assets::resolve_db_path(db.as_deref());
            test_tokens(id, &db_path).await
        }
    }
}

fn handle_service(args: ServiceArgs) -> Result<()> {
    match args.subcommand {
        ServiceSubcommands::Install { bin } => install_user_service(bin.as_deref()),
        ServiceSubcommands::Uninstall => uninstall_user_service(),
        ServiceSubcommands::Status => service_status(),
    }
}

async fn handle_update(args: UpdateArgs) -> Result<()> {
    deeperseeker::cli::update::run_update(args.yes).await
}

fn apply_serve_args(config: &mut AppConfig, args: &ServeArgs) {
    if let Some(h) = &args.host {
        config.host = h.clone();
    }
    if let Some(p) = args.port {
        config.port = p;
    }
    if let Some(d) = &args.db {
        config.db_path = d.clone();
    }
    if let Some(w) = &args.wasm {
        config.wasm_path = deeperseeker::infra::assets::resolve_wasm_path(w);
    }
}

async fn run_server(args: ServeArgs) -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let mut config = AppConfig::from_env();
    apply_serve_args(&mut config, &args);

    let config = Arc::new(config);
    let db = open_db(&config.db_path).await?;
    init_db(&db).await?;

    let _watchdog = deeperseeker::infra::watchdog::start_token_watchdog(
        db.clone(),
        std::time::Duration::from_secs(30),
    );

    let pow_solver = Arc::new(
        PowSolver::new(&config.wasm_path).context("Failed initializing WASM PoW solver engine")?,
    );

    let client = DeepSeekClient::new();
    let template_pattern = deeperseeker::infra::assets::resolve_templates_pattern();
    let tera = Arc::new(Tera::new(&template_pattern).context("Failed compiling HTML templates")?);

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

    let version = env!("CARGO_PKG_VERSION");
    info!("🚀 deeperseeker-rs v{version} listening on http://{bind_addr}");
    info!("OpenAI Base URL: http://{bind_addr}/v1");
    info!("Dashboard:       http://{bind_addr}/dashboard");

    axum::serve(listener, router)
        .await
        .context("Server exited unexpectedly")?;

    Ok(())
}
