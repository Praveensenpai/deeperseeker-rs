use anyhow::{Context, Result};
use std::time::Duration;
use tokio_rusqlite::Connection;
use tracing::info;

/// How often the retention sweep runs.
const SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// Delete stale cached sessions and (optionally) old usage rows.
///
/// A `days` value of 0 disables that sweep. Sessions are keyed by a
/// conversation signature and go stale quickly; usage rows are the historical
/// ledger and are kept forever unless an operator opts in.
///
/// Returns `(sessions_deleted, usage_rows_deleted)`.
pub async fn run_retention(
    conn: &Connection,
    session_days: u64,
    usage_days: u64,
) -> Result<(u64, u64)> {
    let now = crate::infra::db::now_timestamp();
    let session_cutoff = (session_days > 0).then_some(now - (session_days as f64) * 86400.0);
    let usage_cutoff = (usage_days > 0).then_some(now - (usage_days as f64) * 86400.0);

    conn.call(move |c| {
        let sessions = match session_cutoff {
            Some(threshold) => c.execute(
                "DELETE FROM sessions WHERE last_used < ?1",
                rusqlite::params![threshold],
            )? as u64,
            None => 0,
        };
        let usage = match usage_cutoff {
            Some(threshold) => c.execute(
                "DELETE FROM request_usage WHERE timestamp < ?1",
                rusqlite::params![threshold],
            )? as u64,
            None => 0,
        };
        Ok((sessions, usage))
    })
    .await
    .context("Failed running retention sweep")
}

/// Spawn the daily retention sweep. Does nothing when both windows are 0.
pub fn start_retention_task(
    conn: Connection,
    session_days: u64,
    usage_days: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if session_days == 0 && usage_days == 0 {
            return;
        }
        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            ticker.tick().await;
            match run_retention(&conn, session_days, usage_days).await {
                Ok((s, u)) if s > 0 || u > 0 => {
                    info!("Retention sweep removed {s} stale sessions and {u} usage rows");
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Retention sweep error: {e}"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db::{init_db, open_db};
    use crate::infra::usage_db::{init_usage_table, record_usage};

    #[tokio::test]
    async fn retention_prunes_only_when_enabled() {
        let conn = open_db(":memory:").await.expect("open db");
        init_db(&conn).await.expect("init db");
        init_usage_table(&conn).await.expect("init usage");
        record_usage(&conn, "m", 1, 1, 0, None)
            .await
            .expect("usage");

        // Both sweeps disabled: nothing is touched.
        let (s, u) = run_retention(&conn, 0, 0).await.expect("retention");
        assert_eq!((s, u), (0, 0));

        // Session sweep with a huge window removes nothing recent.
        let (s, _) = run_retention(&conn, 365, 0).await.expect("retention");
        assert_eq!(s, 0);

        // Usage sweep enabled with a negative-effective window (1 day) keeps
        // the just-recorded row.
        let (_, u) = run_retention(&conn, 0, 1).await.expect("retention");
        assert_eq!(u, 0);
    }
}
