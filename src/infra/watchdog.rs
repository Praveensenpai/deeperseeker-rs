use crate::infra::db::{get_tokens, mark_active, now_timestamp};
use crate::infra::deepseek_client::DeepSeekClient;
use anyhow::Result;
use std::time::Duration;
use tokio_rusqlite::Connection;
use tracing::info;

pub async fn check_and_recover_tokens(conn: &Connection) -> Result<usize> {
    let tokens = get_tokens(conn).await?;
    let now = now_timestamp();
    let mut recovered = 0;

    for tok in tokens {
        if tok.is_rate_limited() && tok.is_expired_rate_limit(now) {
            mark_active(conn, tok.id).await?;
            let alias = tok.alias.as_deref().unwrap_or("-");
            info!(
                "Token #{} ({}) cooldown expired, watchdog restored to ACTIVE",
                tok.id, alias
            );
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub fn start_token_watchdog(conn: Connection, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(e) = check_and_recover_tokens(&conn).await {
                tracing::warn!("Watchdog token check error: {e}");
            }
        }
    })
}

pub async fn probe_and_recover_suspended(
    conn: &Connection,
    client: &DeepSeekClient,
) -> Result<usize> {
    let tokens = get_tokens(conn).await?;
    let mut recovered = 0;

    for tok in tokens {
        if !tok.is_expired() {
            continue;
        }

        let alias = tok.alias.as_deref().unwrap_or("-");
        tracing::debug!(
            "Probing expired token #{} ({}) for upstream recovery...",
            tok.id,
            alias
        );

        match client
            .create_pow_challenge(&tok.token, "/api/v0/chat/completion")
            .await
        {
            Ok(_) => {
                mark_active(conn, tok.id).await?;
                info!(
                    "Token #{} ({}) verified valid upstream; restored to ACTIVE",
                    tok.id, alias
                );
                recovered += 1;
            }
            Err(e) => {
                tracing::debug!(
                    "Token #{} ({}) probe failed ({:#}); remains EXPIRED",
                    tok.id,
                    alias,
                    e
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(recovered)
}

pub fn start_suspended_token_probe(
    conn: Connection,
    client: DeepSeekClient,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(e) = probe_and_recover_suspended(&conn, &client).await {
                tracing::warn!("Suspended token recovery probe error: {e}");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::db::{add_token, init_db, mark_limited, open_db};

    #[tokio::test]
    async fn test_watchdog_recovers_expired_token() {
        let conn = open_db(":memory:").await.expect("open db");
        init_db(&conn).await.expect("init db");

        add_token(&conn, "token_1", Some("t1"))
            .await
            .expect("add token");
        mark_limited(&conn, 1, 0).await.expect("mark limited");

        let recovered = check_and_recover_tokens(&conn).await.expect("check tokens");
        assert_eq!(recovered, 1);

        let tokens = get_tokens(&conn).await.expect("get tokens");
        assert_eq!(tokens[0].status, "ACTIVE");
    }

    #[tokio::test]
    async fn test_suspended_probe_ignores_active_tokens() {
        let conn = open_db(":memory:").await.expect("open db");
        init_db(&conn).await.expect("init db");

        add_token(&conn, "token_1", Some("t1"))
            .await
            .expect("add token");

        let client = DeepSeekClient::new();
        let recovered = probe_and_recover_suspended(&conn, &client)
            .await
            .expect("probe tokens");
        assert_eq!(recovered, 0);

        let tokens = get_tokens(&conn).await.expect("get tokens");
        assert_eq!(tokens[0].status, "ACTIVE");
    }
}
