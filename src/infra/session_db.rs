use crate::domain::session::Session;
use crate::infra::db::now_timestamp;
use anyhow::{Context, Result};
use rusqlite::params;
use tokio_rusqlite::Connection;

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
