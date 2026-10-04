use crate::domain::usage::{DailyUsage, ModelUsage, TokenUsage, UsageFilter, UsageSummary};
use anyhow::{Context, Result};
use rusqlite::params;
use tokio_rusqlite::Connection;

pub async fn init_usage_table(conn: &Connection) -> Result<()> {
    conn.call(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS request_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp REAL NOT NULL,
                date TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                token_id INTEGER,
                cached_tokens INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_usage_date ON request_usage(date);
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON request_usage(timestamp);",
        )?;
        let _ = c.execute(
            "ALTER TABLE request_usage ADD COLUMN cached_tokens INTEGER DEFAULT 0",
            [],
        );
        Ok(())
    })
    .await
    .context("Failed initializing request_usage table")
}

pub async fn record_usage(
    conn: &Connection,
    model: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
    cached_tokens: u32,
    token_id: Option<i64>,
) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let date_str = time::OffsetDateTime::now_utc()
        .format(&time::macros::format_description!("[year]-[month]-[day]"))
        .unwrap_or_else(|_| "1970-01-01".to_string());

    let m = model.to_string();
    let total = (prompt_tokens + completion_tokens) as i64;
    let p = prompt_tokens as i64;
    let c = completion_tokens as i64;
    let ca = cached_tokens as i64;

    conn.call(move |db| {
        db.execute(
            "INSERT INTO request_usage (timestamp, date, model, prompt_tokens, completion_tokens, total_tokens, token_id, cached_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![now, date_str, m, p, c, total, token_id, ca],
        )?;
        Ok(())
    })
    .await
    .context("Failed recording token usage")
}

fn build_where_clause(base: &str, filter: &UsageFilter) -> String {
    let mut parts = vec![base.to_string()];
    if let Some(m) = &filter.model {
        let escaped = m.replace('\'', "''");
        parts.push(format!("model = '{escaped}'"));
    }
    if let Some(tid) = filter.token_id {
        parts.push(format!("token_id = {tid}"));
    }
    parts.join(" AND ")
}

pub async fn get_all_summaries(conn: &Connection) -> Result<Vec<UsageSummary>> {
    get_filtered_summaries(conn, &UsageFilter::default()).await
}

pub async fn get_filtered_summaries(
    conn: &Connection,
    filter: &UsageFilter,
) -> Result<Vec<UsageSummary>> {
    let periods = [
        ("Today", "date = date('now')"),
        ("Yesterday", "date = date('now', '-1 day')"),
        ("This Week", "date >= date('now', 'weekday 0', '-7 days')"),
        (
            "This Month",
            "strftime('%Y-%m', date) = strftime('%Y-%m', 'now')",
        ),
        ("This Year", "strftime('%Y', date) = strftime('%Y', 'now')"),
        ("All Time", "1=1"),
    ];

    let mut summaries = Vec::new();
    for (name, condition) in periods {
        let where_clause = build_where_clause(condition, filter);
        let summary = fetch_period_summary(conn, name, where_clause).await?;
        summaries.push(summary);
    }
    Ok(summaries)
}

async fn fetch_period_summary(
    conn: &Connection,
    period: &str,
    condition: String,
) -> Result<UsageSummary> {
    let p_name = period.to_string();
    conn.call(move |c| {
        let query = format!(
            "SELECT COUNT(*), COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cached_tokens), 0)
             FROM request_usage WHERE {}",
            condition
        );
        let mut stmt = c.prepare(&query)?;
        let row = stmt.query_row([], |r| {
            let reqs: i64 = r.get(0)?;
            let p: i64 = r.get(1)?;
            let comp: i64 = r.get(2)?;
            let tot: i64 = r.get(3)?;
            let ca: i64 = r.get(4)?;
            Ok(UsageSummary {
                period: p_name,
                requests: reqs as u64,
                prompt_tokens: p as u64,
                completion_tokens: comp as u64,
                total_tokens: tot as u64,
                cached_tokens: ca as u64,
            })
        })?;
        Ok(row)
    })
    .await
    .context("Failed fetching period summary")
}

pub async fn get_daily_breakdown(conn: &Connection, limit: usize) -> Result<Vec<DailyUsage>> {
    get_filtered_daily_breakdown(conn, limit, &UsageFilter::default()).await
}

pub async fn get_filtered_daily_breakdown(
    conn: &Connection,
    limit: usize,
    filter: &UsageFilter,
) -> Result<Vec<DailyUsage>> {
    let where_clause = build_where_clause("1=1", filter);
    conn.call(move |c| {
        let query = format!(
            "SELECT date, COUNT(*), SUM(prompt_tokens), SUM(completion_tokens), SUM(total_tokens), COALESCE(SUM(cached_tokens), 0)
             FROM request_usage
             WHERE {}
             GROUP BY date
             ORDER BY date DESC
             LIMIT ?1",
            where_clause
        );
        let mut stmt = c.prepare(&query)?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            let d: String = r.get(0)?;
            let reqs: i64 = r.get(1)?;
            let p: i64 = r.get(2)?;
            let comp: i64 = r.get(3)?;
            let tot: i64 = r.get(4)?;
            let ca: i64 = r.get(5)?;
            Ok(DailyUsage {
                date: d,
                requests: reqs as u64,
                prompt_tokens: p as u64,
                completion_tokens: comp as u64,
                total_tokens: tot as u64,
                cached_tokens: ca as u64,
            })
        })?;
        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .await
    .context("Failed fetching daily breakdown")
}

pub async fn get_model_breakdown(conn: &Connection) -> Result<Vec<ModelUsage>> {
    get_filtered_model_breakdown(conn, &UsageFilter::default()).await
}

pub async fn get_filtered_model_breakdown(
    conn: &Connection,
    filter: &UsageFilter,
) -> Result<Vec<ModelUsage>> {
    let where_clause = build_where_clause("1=1", filter);
    conn.call(move |c| {
        let query = format!(
            "SELECT model, COUNT(*), SUM(total_tokens)
             FROM request_usage
             WHERE {}
             GROUP BY model
             ORDER BY SUM(total_tokens) DESC",
            where_clause
        );
        let mut stmt = c.prepare(&query)?;
        let rows = stmt.query_map([], |r| {
            let m: String = r.get(0)?;
            let reqs: i64 = r.get(1)?;
            let tot: i64 = r.get(2)?;
            Ok(ModelUsage {
                model: m,
                requests: reqs as u64,
                total_tokens: tot as u64,
            })
        })?;
        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .await
    .context("Failed fetching model breakdown")
}

pub async fn get_token_usages(conn: &Connection) -> Result<Vec<TokenUsage>> {
    conn.call(move |c| {
        let query = "SELECT token_id, COUNT(*), COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cached_tokens), 0)
                     FROM request_usage
                     WHERE token_id IS NOT NULL
                     GROUP BY token_id";
        let mut stmt = c.prepare(query)?;
        let rows = stmt.query_map([], |r| {
            let tid: i64 = r.get(0)?;
            let reqs: i64 = r.get(1)?;
            let p: i64 = r.get(2)?;
            let comp: i64 = r.get(3)?;
            let tot: i64 = r.get(4)?;
            let ca: i64 = r.get(5)?;
            Ok(TokenUsage {
                token_id: tid,
                requests: reqs as u64,
                prompt_tokens: p as u64,
                completion_tokens: comp as u64,
                total_tokens: tot as u64,
                cached_tokens: ca as u64,
            })
        })?;
        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .await
    .context("Failed fetching token usage breakdown")
}
