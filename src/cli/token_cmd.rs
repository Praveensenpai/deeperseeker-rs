use crate::domain::token::Token;
use crate::infra::db::{add_token as db_add, delete_token, get_tokens, init_db, open_db};
use crate::infra::deepseek_client::DeepSeekClient;
use anyhow::{Context, Result};
use std::time::Instant;

fn mask_token(token: &str) -> String {
    if token.len() <= 12 {
        return "***".to_string();
    }
    format!("{}...{}", &token[..6], &token[token.len() - 4..])
}

fn format_timestamp(ts: Option<f64>) -> String {
    match ts {
        Some(t) if t > 0.0 => {
            let secs = t as i64;
            let dt = time::OffsetDateTime::from_unix_timestamp(secs)
                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
            let format =
                time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
            dt.format(&format).unwrap_or_else(|_| "Unknown".to_string())
        }
        _ => "Never".to_string(),
    }
}

pub async fn list_tokens(db_path: &str) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    init_db(&conn)
        .await
        .context("Failed initializing database")?;
    let tokens = get_tokens(&conn)
        .await
        .context("Failed retrieving tokens")?;

    if tokens.is_empty() {
        println!("No tokens configured in database ({}).", db_path);
        println!("Add a token with: deeperseeker token add <USER_TOKEN>");
        return Ok(());
    }

    println!(
        "Pool: {} token(s) configured in {}\n",
        tokens.len(),
        db_path
    );
    println!(
        "{:<4} {:<16} {:<12} {:<22} {:<16}",
        "ID", "Alias", "Status", "Last Used", "Token"
    );
    println!("{:-<4} {:-<16} {:-<12} {:-<22} {:-<16}", "", "", "", "", "");

    for t in &tokens {
        let alias = t.alias.as_deref().unwrap_or("-");
        let last_used = format_timestamp(t.last_used);
        let masked = mask_token(&t.token);
        println!(
            "{:<4} {:<16} {:<12} {:<22} {:<16}",
            t.id, alias, t.status, last_used, masked
        );
    }
    Ok(())
}

pub async fn add_token(token: &str, alias: Option<&str>, db_path: &str) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    db_add(&conn, token, alias)
        .await
        .context("Failed adding token")?;
    println!("✔ Token added successfully to database ({})", db_path);
    Ok(())
}

pub async fn remove_token(token_id: i64, db_path: &str) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    delete_token(&conn, token_id)
        .await
        .context("Failed deleting token")?;
    println!("✔ Token #{} deleted from database", token_id);
    Ok(())
}

pub async fn edit_token(
    token_id: i64,
    new_token: Option<&str>,
    alias: Option<&str>,
    db_path: &str,
) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    init_db(&conn)
        .await
        .context("Failed initializing database")?;

    let exists = get_tokens(&conn)
        .await
        .context("Failed retrieving tokens")?
        .iter()
        .any(|t| t.id == token_id);

    if !exists {
        anyhow::bail!("No token with ID #{token_id} found in database.");
    }

    let trimmed_alias = alias.map(str::trim).filter(|s| !s.is_empty());

    let Some(candidate) = new_token.map(str::trim).filter(|s| !s.is_empty()) else {
        crate::infra::db::update_token(&conn, token_id, None, trimmed_alias, None)
            .await
            .context("Failed to update alias")?;
        println!("✔ Alias updated for token #{token_id}");
        return Ok(());
    };

    let client = DeepSeekClient::new();
    print!("Verifying new token for #{token_id} against upstream... ");
    match client
        .create_pow_challenge(candidate, "/api/v0/chat/completion")
        .await
    {
        Ok(_) => {
            crate::infra::db::update_token(
                &conn,
                token_id,
                Some(candidate),
                trimmed_alias,
                Some("ACTIVE"),
            )
            .await
            .context("Failed to update token")?;
            println!("VALID");
            println!("✔ Token #{token_id} updated and verified (ACTIVE)");
            Ok(())
        }
        Err(e) => {
            crate::infra::db::update_token(&conn, token_id, None, trimmed_alias, Some("EXPIRED"))
                .await
                .context("Failed to mark token expired")?;
            println!("INVALID");
            anyhow::bail!(
                "New token rejected (verification failed): {e:#}. Existing token value was kept."
            )
        }
    }
}

pub async fn test_tokens(target_id: Option<i64>, db_path: &str) -> Result<()> {
    let conn = open_db(db_path).await.context("Failed opening database")?;
    let tokens = get_tokens(&conn)
        .await
        .context("Failed retrieving tokens")?;
    let client = DeepSeekClient::new();

    let candidates: Vec<&Token> = match target_id {
        Some(id) => tokens.iter().filter(|t| t.id == id).collect(),
        None => tokens.iter().collect(),
    };

    if candidates.is_empty() {
        println!("No matching token found to test.");
        return Ok(());
    }

    println!(
        "Testing {} token(s) against DeepSeek upstream...",
        candidates.len()
    );
    for t in candidates {
        test_single_token(&conn, &client, t).await;
    }
    Ok(())
}

async fn test_single_token(
    conn: &tokio_rusqlite::Connection,
    client: &DeepSeekClient,
    token: &Token,
) {
    let masked = mask_token(&token.token);
    print!(
        "  • Token #{} ({}) [{}] ... ",
        token.id,
        token.alias.as_deref().unwrap_or("-"),
        masked
    );

    let start = Instant::now();
    let res = client
        .create_pow_challenge(&token.token, "/api/v0/chat/completion")
        .await;
    let elapsed = start.elapsed();

    match res {
        Ok(challenge) => {
            let _ = crate::infra::db::mark_active(conn, token.id).await;
            println!(
                "VALID (Difficulty: {}, {:.1}ms) -> ACTIVE",
                challenge.difficulty,
                elapsed.as_secs_f64() * 1000.0
            );
        }
        Err(e) => {
            if crate::domain::upstream::is_auth_failure(&e) {
                let _ = crate::infra::db::mark_expired(conn, token.id).await;
                println!("INVALID / EXPIRED ({e:#}) -> EXPIRED");
            } else {
                println!("FAILED ({e:#})");
            }
        }
    }
}
