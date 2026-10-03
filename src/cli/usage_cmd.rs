use crate::domain::usage::format_metric;
use crate::infra::db::{init_db, open_db};
use crate::infra::usage_db::{get_all_summaries, get_daily_breakdown, get_model_breakdown};
use anyhow::{Context, Result};
use serde_json::json;

pub async fn display_usage(db_path: &str, raw: bool, days: usize, as_json: bool) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    init_db(&conn)
        .await
        .context("Failed initializing database")?;
    let summaries = get_all_summaries(&conn)
        .await
        .context("Failed retrieving usage summaries")?;
    let daily = get_daily_breakdown(&conn, days)
        .await
        .context("Failed retrieving daily usage")?;
    let models = get_model_breakdown(&conn)
        .await
        .context("Failed retrieving model usage")?;

    if as_json {
        let out = json!({
            "summaries": summaries,
            "daily": daily,
            "models": models,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    render_summary_table(&summaries, raw);
    render_daily_histogram(&daily, raw);
    render_model_distribution(&models, raw);

    Ok(())
}

fn render_summary_table(summaries: &[crate::domain::usage::UsageSummary], raw: bool) {
    println!("╭─────────────────────────────────────────────────────────────────────────────╮");
    println!("│  📊 DeeperSeeker Token & Request Analytics                                  │");
    println!("├─────────────┬──────────┬──────────────┬──────────────┬──────────────────────┤");
    println!(
        "│ {:<11} │ {:<8} │ {:<12} │ {:<12} │ {:<20} │",
        "Period", "Requests", "Prompt (In)", "Compl (Out)", "Total Tokens"
    );
    println!("├─────────────┼──────────┼──────────────┼──────────────┼──────────────────────┤");

    for s in summaries {
        let reqs = if raw {
            s.requests.to_string()
        } else {
            format_metric(s.requests, false)
        };
        let prompt = format_metric(s.prompt_tokens, raw);
        let compl = format_metric(s.completion_tokens, raw);
        let total = format_metric(s.total_tokens, raw);

        println!(
            "│ {:<11} │ {:<8} │ {:<12} │ {:<12} │ {:<20} │",
            s.period, reqs, prompt, compl, total
        );
    }
    println!("╰─────────────┴──────────┴──────────────┴──────────────┴──────────────────────╯\n");
}

fn render_daily_histogram(daily: &[crate::domain::usage::DailyUsage], raw: bool) {
    if daily.is_empty() {
        return;
    }

    println!("Recent Daily Activity");
    println!("─────────────────────────────────────────────────────────────────────────────");

    let max_tokens = daily.iter().map(|d| d.total_tokens).max().unwrap_or(1);
    let today_str = time::OffsetDateTime::now_utc()
        .format(&time::macros::format_description!("[year]-[month]-[day]"))
        .unwrap_or_default();

    for d in daily {
        let label = if d.date == today_str {
            format!("{} (Today)", d.date)
        } else {
            d.date.clone()
        };

        let ratio = if max_tokens > 0 {
            (d.total_tokens as f64 / max_tokens as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let bar_len = (ratio * 20.0).round() as usize;
        let bar_filled = "█".repeat(bar_len);
        let bar_empty = "░".repeat(20 - bar_len);
        let tok_str = format_metric(d.total_tokens, raw);
        let req_str = if d.requests == 1 { "req" } else { "reqs" };

        println!(
            "{:<23} {}{}  {:>6} tokens  ({} {})",
            label, bar_filled, bar_empty, tok_str, d.requests, req_str
        );
    }
    println!();
}

fn render_model_distribution(models: &[crate::domain::usage::ModelUsage], raw: bool) {
    if models.is_empty() {
        return;
    }

    let total_all: u64 = models.iter().map(|m| m.total_tokens).sum();
    println!("Model Distribution");
    println!("─────────────────────────────────────────────────────────────────────────────");

    for m in models {
        let pct = if total_all > 0 {
            (m.total_tokens as f64 / total_all as f64) * 100.0
        } else {
            0.0
        };
        let tok_str = format_metric(m.total_tokens, raw);
        let req_str = if m.requests == 1 { "req" } else { "reqs" };

        println!(
            "• {:<22} {:>6} tokens ({:>5.1}%)  [{} {}]",
            m.model, tok_str, pct, m.requests, req_str
        );
    }
    println!();
}
