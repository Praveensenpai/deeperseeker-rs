use crate::domain::token::Token;
use anyhow::{Context, Result};
use rusqlite::params;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_rusqlite::Connection;

pub fn now_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub async fn open_db(path: &str) -> Result<Connection> {
    if path != ":memory:" {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
    }
    let conn = Connection::open(path).await.context("Failed to open DB")?;
    conn.call(|c| {
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(())
    })
    .await
    .context("Failed to set WAL mode")?;
    Ok(conn)
}

pub async fn init_db(conn: &Connection) -> Result<()> {
    conn.call(|c| {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS tokens (
                id INTEGER PRIMARY KEY,
                alias TEXT,
                token TEXT UNIQUE,
                status TEXT DEFAULT 'ACTIVE',
                rate_limited_until REAL,
                last_used REAL
            );
            CREATE TABLE IF NOT EXISTS sessions (
                signature TEXT PRIMARY KEY,
                token_id INTEGER,
                deepseek_session_id TEXT,
                parent_message_id INTEGER,
                last_used REAL
            );
            CREATE TABLE IF NOT EXISTS files (
                file_id TEXT PRIMARY KEY,
                token_id INTEGER,
                created_at REAL
            );
            CREATE TABLE IF NOT EXISTS request_usage (
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
            CREATE TABLE IF NOT EXISTS client_keys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                key_prefix TEXT NOT NULL,
                quota_tokens INTEGER NOT NULL DEFAULT 0,
                window_secs INTEGER NOT NULL DEFAULT 0,
                revoked INTEGER NOT NULL DEFAULT 0,
                created_at REAL NOT NULL,
                last_used REAL
            );
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_date ON request_usage(date);
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON request_usage(timestamp);",
        )?;
        // Monotonic token-ID counter: seeded once from the current max so a
        // freshly added token never reuses a deleted token's ID (and thus its
        // usage history), even after the tokens table becomes empty.
        let _ = c.execute(
            "INSERT OR IGNORE INTO meta (key, value)
             SELECT 'next_token_id', COALESCE(MAX(id), 0) + 1 FROM tokens",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE request_usage ADD COLUMN cached_tokens INTEGER DEFAULT 0",
            [],
        );
        let _ = c.execute(
            "ALTER TABLE request_usage ADD COLUMN client_key_id INTEGER",
            [],
        );
        let _ = c.execute(
            "CREATE INDEX IF NOT EXISTS idx_usage_client_key ON request_usage(client_key_id)",
            [],
        );
        // Cascade semantics for tokens deleted before this cleanup existed:
        // drop usage rows whose owning token is gone (AUTOINCREMENT never
        // reuses their IDs, so these can never be re-attributed).
        let _ = c.execute(
            "DELETE FROM request_usage
             WHERE token_id IS NOT NULL
               AND token_id NOT IN (SELECT id FROM tokens)",
            [],
        );
        Ok(())
    })
    .await
    .context("Failed to initialize database tables")?;
    Ok(())
}

pub async fn add_token(conn: &Connection, token: &str, alias: Option<&str>) -> Result<()> {
    let tok = token.to_string();
    let al = alias.map(|s| s.to_string());
    conn.call(move |c| {
        // Persistent monotonic counter: survives deletes and never resets, so
        // a new token can never inherit a deleted token's ID / usage history.
        let next_id: i64 = c
            .query_row(
                "SELECT value FROM meta WHERE key = 'next_token_id'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| {
                c.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM tokens", [], |r| r.get(0))
                    .unwrap_or(1)
            });
        c.execute(
            "INSERT INTO tokens (id, alias, token, status, last_used) VALUES (?1, ?2, ?3, 'ACTIVE', ?4)",
            params![next_id, al, tok, now_timestamp()],
        )?;
        c.execute(
            "INSERT INTO meta (key, value) VALUES ('next_token_id', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![next_id + 1],
        )?;
        Ok(())
    })
    .await
    .context("Failed to insert token")?;
    Ok(())
}

/// Update a token's credentials, alias and/or status.
///
/// Alias and status are labels only; changing them never touches usage history.
/// Usage is cleared only when the token is deleted (cascade) or via the
/// explicit [`reset_token_usage`] action.
pub async fn update_token(
    conn: &Connection,
    token_id: i64,
    new_token: Option<&str>,
    alias: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let tok = new_token.map(str::to_string);
    let al = alias.map(str::to_string);
    let st = status.map(str::to_string);
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET token = COALESCE(?1, token), alias = COALESCE(?2, alias), status = COALESCE(?3, status) WHERE id = ?4",
            params![tok, al, st, token_id],
        )?;
        if st.as_deref() == Some("ACTIVE") {
            c.execute(
                "UPDATE tokens SET rate_limited_until = NULL WHERE id = ?1",
                params![token_id],
            )?;
        }
        Ok(())
    })
    .await
    .context("Failed to update token")?;
    Ok(())
}

/// Clear a token's recorded usage and cached sessions. Returns (usage_rows,
/// session_rows) deleted. Manual counterpart to the cascade that runs when a
/// token is deleted, for resetting a token in place.
pub async fn reset_token_usage(conn: &Connection, token_id: i64) -> Result<(u64, u64)> {
    conn.call(move |c| {
        let usage = c.execute(
            "DELETE FROM request_usage WHERE token_id = ?1",
            params![token_id],
        )? as u64;
        let sessions = c.execute(
            "DELETE FROM sessions WHERE token_id = ?1",
            params![token_id],
        )? as u64;
        Ok((usage, sessions))
    })
    .await
    .context("Failed to reset token usage")
}

pub async fn get_tokens(conn: &Connection) -> Result<Vec<Token>> {
    conn.call(|c| {
        let mut stmt = c.prepare(
            "SELECT id, alias, token, status, rate_limited_until, last_used FROM tokens ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Token {
                id: r.get(0)?,
                alias: r.get(1)?,
                token: r.get(2)?,
                status: r.get(3)?,
                rate_limited_until: r.get(4)?,
                last_used: r.get(5)?,
            })
        })?;
        let mut list = Vec::new();
        for item in rows {
            list.push(item?);
        }
        Ok(list)
    })
    .await
    .context("Failed to fetch tokens")
}

pub async fn get_token(conn: &Connection, token_id: i64) -> Result<Option<Token>> {
    conn.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT id, alias, token, status, rate_limited_until, last_used FROM tokens WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![token_id], |r| {
            Ok(Token {
                id: r.get(0)?,
                alias: r.get(1)?,
                token: r.get(2)?,
                status: r.get(3)?,
                rate_limited_until: r.get(4)?,
                last_used: r.get(5)?,
            })
        })?;
        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    })
    .await
    .context("Failed to fetch token by id")
}

pub async fn delete_token(conn: &Connection, token_id: i64) -> Result<()> {
    conn.call(move |c| {
        c.execute("DELETE FROM tokens WHERE id = ?1", params![token_id])?;
        c.execute(
            "DELETE FROM sessions WHERE token_id = ?1",
            params![token_id],
        )?;
        c.execute(
            "DELETE FROM request_usage WHERE token_id = ?1",
            params![token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to delete token")?;
    Ok(())
}

pub async fn mark_limited(conn: &Connection, token_id: i64, cooldown_secs: u64) -> Result<()> {
    let until = now_timestamp() + (cooldown_secs as f64);
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET status = 'RATE_LIMITED', rate_limited_until = ?1 WHERE id = ?2",
            params![until, token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to mark token rate limited")?;
    Ok(())
}

pub async fn mark_active(conn: &Connection, token_id: i64) -> Result<()> {
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET status = 'ACTIVE', rate_limited_until = NULL WHERE id = ?1",
            params![token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to mark token active")?;
    Ok(())
}

pub async fn mark_expired(conn: &Connection, token_id: i64) -> Result<()> {
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET status = 'EXPIRED', rate_limited_until = NULL WHERE id = ?1",
            params![token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to mark token expired")?;
    Ok(())
}

pub async fn mark_suspended(conn: &Connection, token_id: i64) -> Result<()> {
    mark_expired(conn, token_id).await
}

pub async fn touch_token(conn: &Connection, token_id: i64) -> Result<()> {
    let now = now_timestamp();
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET last_used = ?1 WHERE id = ?2",
            params![now, token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to touch token")?;
    Ok(())
}

pub async fn pick_token(
    conn: &Connection,
    exclude: &[i64],
    in_flight: &HashMap<i64, usize>,
    concurrency_cap: usize,
) -> Result<Option<Token>> {
    let tokens = get_tokens(conn).await?;
    let now = now_timestamp();
    let mut candidates = Vec::new();

    for tok in tokens {
        if exclude.contains(&tok.id) {
            continue;
        }
        if tok.is_rate_limited() && tok.is_expired_rate_limit(now) {
            mark_active(conn, tok.id).await?;
            let mut active_tok = tok.clone();
            active_tok.status = "ACTIVE".to_string();
            candidates.push(active_tok);
        } else if tok.is_active() {
            candidates.push(tok);
        }
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    candidates.sort_by(|a, b| {
        let inf_a = in_flight.get(&a.id).copied().unwrap_or(0);
        let inf_b = in_flight.get(&b.id).copied().unwrap_or(0);
        let under_cap_a = inf_a < concurrency_cap;
        let under_cap_b = inf_b < concurrency_cap;

        match (under_cap_a, under_cap_b) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => inf_a.cmp(&inf_b).then_with(|| {
                a.last_used
                    .unwrap_or(0.0)
                    .total_cmp(&b.last_used.unwrap_or(0.0))
            }),
        }
    });

    Ok(candidates.into_iter().next())
}
