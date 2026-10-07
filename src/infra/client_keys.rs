//! Storage and quota accounting for downstream client API keys.
//!
//! Keys are stored as SHA-256 hashes. The plaintext is returned exactly once,
//! at creation time, and is unrecoverable afterwards.

use crate::domain::client_key::ClientKey;
use anyhow::{Context, Result};
use rand::RngCore;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tokio_rusqlite::Connection;

/// Prefix that identifies keys issued by this service.
const KEY_PREFIX: &str = "dsk-";
const KEY_BYTES: usize = 24;
/// Characters of the plaintext key kept for display in the dashboard.
const DISPLAY_PREFIX_LEN: usize = 12;

const SELECT_COLUMNS: &str =
    "id, name, key_prefix, quota_tokens, window_secs, revoked, created_at, last_used";

/// Generate a fresh plaintext API key. Callers must show it once and drop it.
pub fn generate_key() -> String {
    let mut bytes = [0u8; KEY_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{KEY_PREFIX}{}", hex::encode(bytes))
}

pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

fn display_prefix(key: &str) -> String {
    key.chars().take(DISPLAY_PREFIX_LEN).collect()
}

fn row_to_client_key(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClientKey> {
    Ok(ClientKey {
        id: row.get(0)?,
        name: row.get(1)?,
        key_prefix: row.get(2)?,
        quota_tokens: row.get::<_, i64>(3)?.max(0) as u64,
        window_secs: row.get::<_, i64>(4)?.max(0) as u64,
        revoked: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        last_used: row.get(7)?,
    })
}

/// Insert a new key. Returns its ID and the plaintext key (shown once).
pub async fn add_client_key(
    conn: &Connection,
    name: &str,
    quota_tokens: u64,
    window_secs: u64,
) -> Result<(i64, String)> {
    let plaintext = generate_key();
    let hash = hash_key(&plaintext);
    let prefix = display_prefix(&plaintext);
    let name = name.to_string();
    let now = crate::infra::db::now_timestamp();

    let id = conn
        .call(move |c| {
            c.execute(
                "INSERT INTO client_keys
                 (name, key_hash, key_prefix, quota_tokens, window_secs, revoked, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
                params![
                    name,
                    hash,
                    prefix,
                    quota_tokens as i64,
                    window_secs as i64,
                    now
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await
        .context("Failed inserting client key")?;

    Ok((id, plaintext))
}

pub async fn get_client_key(conn: &Connection, id: i64) -> Result<Option<ClientKey>> {
    conn.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM client_keys WHERE id = ?1"
        ))?;
        let mut rows = stmt.query_map(params![id], row_to_client_key)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    })
    .await
    .context("Failed fetching client key")
}

/// Look up a non-revoked key by its plaintext value.
pub async fn find_active_by_plaintext(
    conn: &Connection,
    plaintext: &str,
) -> Result<Option<ClientKey>> {
    let hash = hash_key(plaintext);
    conn.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM client_keys WHERE key_hash = ?1 AND revoked = 0"
        ))?;
        let mut rows = stmt.query_map(params![hash], row_to_client_key)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    })
    .await
    .context("Failed looking up client key")
}

pub async fn list_client_keys(conn: &Connection) -> Result<Vec<ClientKey>> {
    conn.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM client_keys ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map([], row_to_client_key)?;
        let mut keys = Vec::new();
        for row in rows {
            keys.push(row?);
        }
        Ok(keys)
    })
    .await
    .context("Failed listing client keys")
}

pub async fn set_revoked(conn: &Connection, id: i64, revoked: bool) -> Result<()> {
    conn.call(move |c| {
        c.execute(
            "UPDATE client_keys SET revoked = ?1 WHERE id = ?2",
            params![revoked as i64, id],
        )?;
        Ok(())
    })
    .await
    .context("Failed updating client key revocation")
}

pub async fn delete_client_key(conn: &Connection, id: i64) -> Result<()> {
    conn.call(move |c| {
        c.execute("DELETE FROM client_keys WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .context("Failed deleting client key")
}

pub async fn touch_client_key(conn: &Connection, id: i64) -> Result<()> {
    let now = crate::infra::db::now_timestamp();
    conn.call(move |c| {
        c.execute(
            "UPDATE client_keys SET last_used = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    })
    .await
    .context("Failed touching client key")
}

/// Sum of total tokens consumed by a key inside its rolling window.
pub async fn window_usage_tokens(
    conn: &Connection,
    client_key_id: i64,
    window_secs: u64,
) -> Result<u64> {
    let cutoff = if window_secs == 0 {
        0.0
    } else {
        crate::infra::db::now_timestamp() - window_secs as f64
    };
    conn.call(move |c| {
        let used: i64 = c.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM request_usage
             WHERE client_key_id = ?1 AND timestamp >= ?2",
            params![client_key_id, cutoff],
            |r| r.get(0),
        )?;
        Ok(used.max(0) as u64)
    })
    .await
    .context("Failed computing client key window usage")
}
