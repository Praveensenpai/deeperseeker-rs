use crate::domain::session::Session;
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
    .context("Failed to initialize database tables")?;
    Ok(())
}

pub async fn add_token(conn: &Connection, token: &str, alias: Option<&str>) -> Result<()> {
    let tok = token.to_string();
    let al = alias.map(|s| s.to_string());
    conn.call(move |c| {
        let mut stmt = c.prepare("SELECT id FROM tokens ORDER BY id ASC")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id?);
        }
        let mut target_id = 1;
        for id in ids {
            if id == target_id {
                target_id += 1;
            } else if id > target_id {
                break;
            }
        }
        c.execute(
            "INSERT INTO tokens (id, alias, token, status, last_used) VALUES (?1, ?2, ?3, 'ACTIVE', ?4)",
            params![target_id, al, tok, now_timestamp()],
        )?;
        Ok(())
    })
    .await
    .context("Failed to insert token")?;
    Ok(())
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

pub async fn mark_suspended(conn: &Connection, token_id: i64) -> Result<()> {
    conn.call(move |c| {
        c.execute(
            "UPDATE tokens SET status = 'SUSPENDED', rate_limited_until = NULL WHERE id = ?1",
            params![token_id],
        )?;
        Ok(())
    })
    .await
    .context("Failed to mark token suspended")?;
    Ok(())
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

pub async fn find_session(conn: &Connection, signature: &str) -> Result<Option<Session>> {
    let sig = signature.to_string();
    let now = now_timestamp();
    conn.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT token_id, deepseek_session_id, parent_message_id, last_used FROM sessions WHERE signature = ?1",
        )?;
        let mut rows = stmt.query_map(params![sig], |r| {
            Ok(Session {
                token_id: r.get(0)?,
                session_id: r.get(1)?,
                parent_message_id: r.get(2)?,
                last_used: r.get(3)?,
            })
        })?;
        if let Some(item) = rows.next() {
            c.execute(
                "UPDATE sessions SET last_used = ?1 WHERE signature = ?2",
                params![now, sig],
            )?;
            Ok(Some(item?))
        } else {
            Ok(None)
        }
    })
    .await
    .context("Failed to find session")
}

pub async fn save_session(conn: &Connection, signature: &str, session: &Session) -> Result<()> {
    let sig = signature.to_string();
    let sess = session.clone();
    let now = now_timestamp();
    conn.call(move |c| {
        c.execute(
            "INSERT INTO sessions (signature, token_id, deepseek_session_id, parent_message_id, last_used)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(signature) DO UPDATE SET
                token_id = excluded.token_id,
                deepseek_session_id = excluded.deepseek_session_id,
                parent_message_id = excluded.parent_message_id,
                last_used = excluded.last_used",
            params![sig, sess.token_id, sess.session_id, sess.parent_message_id, now],
        )?;
        Ok(())
    })
    .await
    .context("Failed to save session")?;
    Ok(())
}

pub async fn delete_sessions_for_chat(
    conn: &Connection,
    token_id: i64,
    session_id: &str,
) -> Result<()> {
    let sid = session_id.to_string();
    conn.call(move |c| {
        c.execute(
            "DELETE FROM sessions WHERE token_id = ?1 AND deepseek_session_id = ?2",
            params![token_id, sid],
        )?;
        Ok(())
    })
    .await
    .context("Failed to delete chat sessions")?;
    Ok(())
}

pub async fn record_file(conn: &Connection, file_id: &str, token_id: i64) -> Result<()> {
    let fid = file_id.to_string();
    let now = now_timestamp();
    conn.call(move |c| {
        c.execute(
            "INSERT OR IGNORE INTO files (file_id, token_id, created_at) VALUES (?1, ?2, ?3)",
            params![fid, token_id, now],
        )?;
        Ok(())
    })
    .await
    .context("Failed to record file")?;
    Ok(())
}

pub async fn get_file_token(conn: &Connection, file_id: &str) -> Result<Option<i64>> {
    let fid = file_id.to_string();
    conn.call(move |c| {
        let mut stmt = c.prepare("SELECT token_id FROM files WHERE file_id = ?1")?;
        let mut rows = stmt.query_map(params![fid], |r| r.get::<_, i64>(0))?;
        if let Some(item) = rows.next() {
            Ok(Some(item?))
        } else {
            Ok(None)
        }
    })
    .await
    .context("Failed to get file token")
}
