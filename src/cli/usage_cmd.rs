use crate::domain::usage::{format_metric, UsageFilter};
use crate::infra::db::{get_tokens, init_db, open_db};
use crate::infra::usage_db::{
    get_filtered_daily_breakdown, get_filtered_model_breakdown, get_filtered_summaries,
};
use anyhow::{Context, Result};
use serde_json::json;

pub struct UsageViewArgs {
    pub db_path: String,
    pub raw: bool,
    pub days: usize,
    pub as_json: bool,
    pub model: Option<String>,
    pub token: Option<String>,
}

async fn resolve_filter_token(
    conn: &tokio_rusqlite::Connection,
    tok_str: &str,
) -> Result<Option<i64>> {
    let tokens = get_tokens(conn).await?;
    if let Some(t) = tokens
        .iter()
        .find(|t| t.alias.as_deref() == Some(tok_str) || t.id.to_string() == *tok_str)
    {
        return Ok(Some(t.id));
    }
    if let Ok(parsed) = tok_str.parse::<i64>() {
        return Ok(Some(parsed));
    }
    Ok(None)
}

pub async fn display_usage(args: UsageViewArgs) -> Result<()> {
    let conn = open_db(&args.db_path)
        .await
        .context("Failed opening database")?;
    init_db(&conn)
        .await
        .context("Failed initializing database")?;

    let mut token_id = None;
    if let Some(tok_str) = &args.token {
        match resolve_filter_token(&conn, tok_str).await? {
            Some(id) => token_id = Some(id),
            None => {
                println!("No token matching '{tok_str}' found in database.");
                return Ok(());
            }
        }
    }

    let filter = UsageFilter {
        model: args.model.clone(),
        token_id,
    };

    let summaries = get_filtered_summaries(&conn, &filter).await?;
    let daily = get_filtered_daily_breakdown(&conn, args.days, &filter).await?;
    let models = get_filtered_model_breakdown(&conn, &filter).await?;

    if args.as_json {
        let out = json!({
            "filter": { "model": args.model, "token_id": token_id },
            "summaries": summaries,
            "daily": daily,
            "models": models,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if args.model.is_some() || args.token.is_some() {
        let m_disp = args.model.as_deref().unwrap_or("All");
        let t_disp = args.token.as_deref().unwrap_or("All");
        println!("Filters applied -> Model: {m_disp} | Token: {t_disp}\n");
    }

    render_summary_table(&summaries, args.raw);
    render_daily_histogram(&daily, args.raw);
    render_model_distribution(&models, args.raw);

    Ok(())
}

fn render_summary_table(summaries: &[crate::domain::usage::UsageSummary], raw: bool) {
    println!("╭─────────────────────────────────────────────────────────────────────────────────────────╮");
    println!("│  📊 DeeperSeeker Token & Request Analytics                                              │");
    println!("├─────────────┬──────────┬──────────────┬──────────────┬──────────────┬───────────────────┤");
    println!(
        "│ {:<11} │ {:<8} │ {:<12} │ {:<12} │ {:<12} │ {:<17} │",
        "Period", "Requests", "Input (In)", "Cached (Save)", "Output (Out)", "Total Tokens"
    );
    println!("├─────────────┼──────────┼──────────────┼──────────────┼──────────────┼───────────────────┤");

    for s in summaries {
        let reqs = if raw {
            s.requests.to_string()
        } else {
            format_metric(s.requests, false)
        };
        let prompt = format_metric(s.prompt_tokens, raw);
        let cached = format_metric(s.cached_tokens, raw);
        let compl = format_metric(s.completion_tokens, raw);
        let total = format_metric(s.total_tokens, raw);

        println!(
            "│ {:<11} │ {:<8} │ {:<12} │ {:<12} │ {:<12} │ {:<17} │",
            s.period, reqs, prompt, cached, compl, total
        );
    }
    println!("╰─────────────┴──────────┴──────────────┴──────────────┴──────────────┴───────────────────╯\n");
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
