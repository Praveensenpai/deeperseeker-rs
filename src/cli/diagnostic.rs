use crate::domain::upstream::PowChallenge;
use crate::infra::db::{get_tokens, init_db, open_db};
use crate::infra::pow::PowSolver;
use anyhow::{Context, Result};
use std::time::Instant;

pub async fn run_diagnostics(
    db_path: &str,
    wasm_path: &str,
    server_url: Option<&str>,
) -> Result<()> {
    println!("🔍 Running DeeperSeeker System Diagnostics...\n");

    check_database(db_path).await?;
    benchmark_pow_solver(wasm_path)?;
    check_upstream_connectivity().await?;

    if let Some(url) = server_url {
        check_local_server(url).await?;
    }

    println!("\n✔ All diagnostic probes completed successfully.");
    Ok(())
}

async fn check_database(db_path: &str) -> Result<()> {
    print!("  [1/4] SQLite Database ({})... ", db_path);
    let start = Instant::now();
    let conn = open_db(db_path).await.context("Database open failed")?;
    init_db(&conn).await.context("Database init failed")?;
    let tokens = get_tokens(&conn).await.context("Failed fetching tokens")?;
    let elapsed = start.elapsed();

    let active_count = tokens.iter().filter(|t| t.is_active()).count();
    println!("OK ({:.1}ms)", elapsed.as_secs_f64() * 1000.0);
    println!(
        "        Total tokens: {} | Active: {} | Suspended: {}",
        tokens.len(),
        active_count,
        tokens.len() - active_count
    );
    Ok(())
}

fn benchmark_pow_solver(wasm_path: &str) -> Result<()> {
    print!("  [2/4] Wasmtime PoW Engine ({})... ", wasm_path);
    let solver = PowSolver::new(wasm_path).context("PoW solver initialization failed")?;

    let challenge = PowChallenge {
        algorithm: Some("DeepSeekHashV1".to_string()),
        challenge: "56792d7dd642aa1c27191ba9710df5ceb3504cd3a55b225b13f6dad53356f560".to_string(),
        salt: "4e8565ce57578f4efb4d".to_string(),
        difficulty: 144000,
        expire_at: 1_791_027_488_061,
        signature: "benchmark_sig".to_string(),
        target_path: Some("/api/v0/chat/completion".to_string()),
    };

    let start = Instant::now();
    let _answer = solver
        .solve(&challenge, "/api/v0/chat/completion")
        .context("PoW solver execution failed")?;
    let elapsed = start.elapsed();

    println!("OK in {:.2}ms", elapsed.as_secs_f64() * 1000.0);
    Ok(())
}

async fn check_upstream_connectivity() -> Result<()> {
    print!("  [3/4] Upstream DeepSeek Gateway (chat.deepseek.com)... ");
    let client = reqwest::Client::new();
    let start = Instant::now();
    let resp = client
        .get("https://chat.deepseek.com")
        .send()
        .await
        .context("Failed to connect to DeepSeek upstream")?;
    let elapsed = start.elapsed();

    println!(
        "OK (HTTP {}, {:.1}ms)",
        resp.status(),
        elapsed.as_secs_f64() * 1000.0
    );
    Ok(())
}

async fn check_local_server(url: &str) -> Result<()> {
    print!("  [4/4] Local DeeperSeeker Proxy ({})... ", url);
    let target = format!("{}/health", url.trim_end_matches('/'));
    let start = Instant::now();
    let client = reqwest::Client::new();
    let resp = client.get(&target).send().await;
    let elapsed = start.elapsed();

    match resp {
        Ok(r) if r.status().is_success() => {
            println!(
                "OK (HTTP {}, {:.1}ms)",
                r.status(),
                elapsed.as_secs_f64() * 1000.0
            );
        }
        Ok(r) => {
            println!(
                "WARN (HTTP {}, {:.1}ms)",
                r.status(),
                elapsed.as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            println!("OFFLINE ({})", e);
        }
    }
    Ok(())
}
