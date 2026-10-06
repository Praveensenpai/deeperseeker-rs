use clap::{Args, Parser, Subcommand};

pub mod diagnostic;
pub mod service;
pub mod token_cmd;
pub mod update;
pub mod usage_cmd;

#[derive(Parser, Debug)]
#[command(
    name = "deeperseeker",
    author = "Praveensenpai",
    version = env!("CARGO_PKG_VERSION"),
    about = "High-Performance DeepSeek AI Proxy Gateway in Rust"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the DeepSeek proxy server (default)
    Serve(ServeArgs),
    /// Launch interactive Ratatui TUI dashboard or print live health snapshot
    Status(StatusArgs),
    /// View token usage analytics (Today, Yesterday, Week, Month, Year, Date-wise)
    #[command(alias = "stats")]
    Usage(UsageArgs),
    /// Manage DeepSeek tokens in the pool
    Token(TokenArgs),
    /// Run diagnostic self-test (DB, PoW WASM solver, upstream API)
    Test(TestArgs),
    /// Manage systemd user service (Linux)
    Service(ServiceArgs),
    /// Download and install the latest release binary from GitHub
    Update(UpdateArgs),
}

#[derive(Args, Debug, Default)]
pub struct ServeArgs {
    /// Host to bind server to
    #[arg(short = 'b', long)]
    pub host: Option<String>,
    /// Port to listen on
    #[arg(short = 'p', long)]
    pub port: Option<u16>,
    /// Path to SQLite database file
    #[arg(short = 'd', long)]
    pub db: Option<String>,
    /// Path to WASM PoW solver file
    #[arg(short = 'w', long)]
    pub wasm: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Base URL of deeperseeker server to probe
    #[arg(long, default_value = "http://127.0.0.1:4000")]
    pub url: String,
    /// Path to SQLite database file
    #[arg(short = 'd', long)]
    pub db: Option<String>,
    /// Output plain text snapshot instead of launching interactive Ratatui TUI
    #[arg(long)]
    pub plain: bool,
}

#[derive(Args, Debug)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub subcommand: TokenSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum TokenSubcommands {
    /// List all tokens registered in the database
    List {
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
    /// Add a new DeepSeek token to the pool
    Add {
        /// Token string (userToken from chat.deepseek.com)
        token: String,
        /// Optional human-readable alias
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
    /// Remove a token from the pool by ID
    Remove {
        /// Token ID to remove
        id: i64,
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
    /// Edit a token's value and/or alias (new token is verified before saving)
    Edit {
        /// Token ID to edit
        id: i64,
        /// New token string (userToken from chat.deepseek.com)
        #[arg(short = 't', long)]
        token: Option<String>,
        /// New human-readable alias
        #[arg(short = 'a', long)]
        alias: Option<String>,
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
    /// Clear a token's recorded usage history and cached sessions
    Reset {
        /// Token ID whose usage should be cleared
        id: i64,
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
    /// Test token validity against upstream DeepSeek API
    Test {
        /// Optional token ID to test (tests all if omitted)
        id: Option<i64>,
        #[arg(short = 'd', long)]
        db: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct TestArgs {
    #[arg(short = 'd', long)]
    pub db: Option<String>,
    #[arg(short = 'w', long, default_value = "wasm/deepseek_pow_solver.wasm")]
    pub wasm: String,
    #[arg(long, default_value = "http://127.0.0.1:4000")]
    pub url: String,
}

#[derive(Args, Debug)]
pub struct ServiceArgs {
    #[command(subcommand)]
    pub subcommand: ServiceSubcommands,
}

#[derive(Subcommand, Debug)]
pub enum ServiceSubcommands {
    /// Install and enable systemd --user service
    Install {
        /// Custom path to deeperseeker binary
        #[arg(long)]
        bin: Option<String>,
    },
    /// Stop, disable, and uninstall systemd --user service
    Uninstall,
    /// Check systemd service status
    Status,
}

#[derive(Args, Debug)]
pub struct UsageArgs {
    /// Path to SQLite database file
    #[arg(short = 'd', long)]
    pub db: Option<String>,
    /// Number of recent days to display in daily activity
    #[arg(long, default_value_t = 7)]
    pub days: usize,
    /// Display raw unrounded token numbers instead of K, M, B
    #[arg(long)]
    pub raw: bool,
    /// Output raw JSON payload instead of styled table
    #[arg(long)]
    pub json: bool,
    /// Filter analytics by model name
    #[arg(short = 'm', long)]
    pub model: Option<String>,
    /// Filter analytics by token alias or numeric ID
    #[arg(short = 't', long)]
    pub token: Option<String>,
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Skip confirmation prompt and apply update immediately
    #[arg(short = 'y', long)]
    pub yes: bool,
}
