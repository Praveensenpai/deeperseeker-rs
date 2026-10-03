use crate::domain::token::Token;
use crate::domain::usage::{format_metric, UsageSummary};
use crate::infra::db::{get_tokens, open_db};
use crate::infra::pow::PowSolver;
use crate::infra::usage_db::get_all_summaries;
use crate::tui::views::{render_ui, ActiveTab, RenderState};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serde_json::Value;
use std::io::{stdout, IsTerminal};
use std::time::{Duration, Instant};

pub mod tabs;
pub mod views;

pub struct TuiData {
    pub server_online: bool,
    pub server_version: String,
    pub tokens: Vec<Token>,
    pub summaries: Vec<UsageSummary>,
    pub in_flight: usize,
    pub pow_latency_ms: f64,
}

enum KeyAction {
    Continue,
    Refresh,
    Quit,
}

pub async fn run_status(server_url: &str, db_path: &str, plain: bool) -> Result<()> {
    if plain || !stdout().is_terminal() {
        return render_plain_status(server_url, db_path).await;
    }
    run_interactive_tui(server_url, db_path).await
}

pub async fn render_plain_status(server_url: &str, db_path: &str) -> Result<()> {
    let data = fetch_status_data(server_url, db_path).await;
    let status_str = if data.server_online {
        "🟢 ONLINE"
    } else {
        "🔴 OFFLINE"
    };

    let active_count = data.tokens.iter().filter(|t| t.is_active()).count();
    println!("=== ⏱️ deeperseeker status ===");
    println!("Gateway:     {} ({})", server_url, status_str);
    println!("Version:     {}", data.server_version);
    println!(
        "Token Pool:  {} Total | {} Active | {} Inactive",
        data.tokens.len(),
        active_count,
        data.tokens.len() - active_count
    );
    if let Some(today) = data.summaries.iter().find(|s| s.period == "Today") {
        println!(
            "Today:       {} req(s) | {} total tokens",
            today.requests,
            format_metric(today.total_tokens, false)
        );
    }
    println!("In-Flight:   {} active request(s)", data.in_flight);
    println!(
        "PoW Engine:  {:.2}ms benchmark (Wasmtime 28)",
        data.pow_latency_ms
    );
    println!("Database:    {}", db_path);
    Ok(())
}

async fn query_server_health(server_url: &str) -> (bool, String, usize) {
    let health_url = format!("{}/health", server_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .unwrap_or_default();

    let Ok(resp) = client.get(&health_url).send().await else {
        return (false, env!("CARGO_PKG_VERSION").to_string(), 0);
    };
    if !resp.status().is_success() {
        return (false, env!("CARGO_PKG_VERSION").to_string(), 0);
    }
    let Ok(json) = resp.json::<Value>().await else {
        return (true, env!("CARGO_PKG_VERSION").to_string(), 0);
    };

    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_string();
    let in_flight = json.get("in_flight").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    (true, version, in_flight)
}

async fn fetch_status_data(server_url: &str, db_path: &str) -> TuiData {
    let (online, version, in_flight) = query_server_health(server_url).await;

    let (tokens, summaries) = match open_db(db_path).await {
        Ok(conn) => {
            let toks = get_tokens(&conn).await.unwrap_or_default();
            let sums = get_all_summaries(&conn).await.unwrap_or_default();
            (toks, sums)
        }
        Err(_) => (Vec::new(), Vec::new()),
    };

    let pow_latency_ms = measure_pow_latency();

    TuiData {
        server_online: online,
        server_version: version,
        tokens,
        summaries,
        in_flight,
        pow_latency_ms,
    }
}

fn measure_pow_latency() -> f64 {
    let wasm_path = "wasm/deepseek_pow_solver.wasm";
    let Ok(solver) = PowSolver::new(wasm_path) else {
        return 0.0;
    };

    let challenge = crate::domain::upstream::PowChallenge {
        algorithm: Some("DeepSeekHashV1".to_string()),
        challenge: "56792d7dd642aa1c27191ba9710df5ceb3504cd3a55b225b13f6dad53356f560".to_string(),
        salt: "4e8565ce57578f4efb4d".to_string(),
        difficulty: 144000,
        expire_at: 1_791_027_488_061,
        signature: "sig".to_string(),
        target_path: Some("/api/v0/chat/completion".to_string()),
    };
    let start = Instant::now();
    if solver.solve(&challenge, "/api/v0/chat/completion").is_ok() {
        start.elapsed().as_secs_f64() * 1000.0
    } else {
        0.0
    }
}

async fn run_interactive_tui(server_url: &str, db_path: &str) -> Result<()> {
    enable_raw_mode().context("Failed enabling terminal raw mode")?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).context("Failed entering alternate screen")?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).context("Failed initializing Ratatui terminal")?;

    let res = tui_loop(&mut terminal, server_url, db_path).await;

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    res
}

fn poll_key_event() -> Option<KeyCode> {
    if !event::poll(Duration::from_millis(250)).unwrap_or(false) {
        return None;
    }
    match event::read() {
        Ok(Event::Key(k)) => Some(k.code),
        _ => None,
    }
}

fn handle_key(code: KeyCode, active_tab: &mut ActiveTab, is_paused: &mut bool) -> KeyAction {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => KeyAction::Quit,
        KeyCode::Tab => {
            *active_tab = active_tab.next();
            KeyAction::Continue
        }
        KeyCode::Char('p') => {
            *is_paused = !*is_paused;
            KeyAction::Continue
        }
        KeyCode::Char('r') => KeyAction::Refresh,
        _ => KeyAction::Continue,
    }
}

async fn tui_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    server_url: &str,
    db_path: &str,
) -> Result<()> {
    let mut active_tab = ActiveTab::Monitor;
    let mut is_paused = false;
    let mut last_poll = Instant::now();
    let mut data = fetch_status_data(server_url, db_path).await;

    loop {
        if !is_paused && last_poll.elapsed() >= Duration::from_secs(2) {
            data = fetch_status_data(server_url, db_path).await;
            last_poll = Instant::now();
        }

        let now_str = time::OffsetDateTime::now_utc()
            .format(&time::macros::format_description!(
                "[hour]:[minute]:[second] UTC"
            ))
            .unwrap_or_else(|_| "Now".to_string());

        let active_count = data.tokens.iter().filter(|t| t.is_active()).count();
        let render_state = RenderState {
            server_online: data.server_online,
            server_version: &data.server_version,
            server_url,
            active_tab,
            tokens: &data.tokens,
            summaries: &data.summaries,
            active_count,
            in_flight_count: data.in_flight,
            pow_latency_ms: data.pow_latency_ms,
            is_paused,
            last_updated: &now_str,
        };

        terminal
            .draw(|f| render_ui(f, &render_state))
            .context("Drawing TUI failed")?;

        let Some(code) = poll_key_event() else {
            continue;
        };

        match handle_key(code, &mut active_tab, &mut is_paused) {
            KeyAction::Quit => break,
            KeyAction::Refresh => {
                data = fetch_status_data(server_url, db_path).await;
                last_poll = Instant::now();
            }
            KeyAction::Continue => {}
        }
    }
    Ok(())
}
