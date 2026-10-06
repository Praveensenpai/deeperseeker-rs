use crate::infra::db::get_token;
use crate::infra::deepseek_client::DeepSeekClient;
use crate::infra::pow::PowSolver;
use crate::infra::session_db::{get_file_token, record_file};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio_rusqlite::Connection;

pub async fn rehome_foreign_files(
    db: &Connection,
    client: &DeepSeekClient,
    solver: &Arc<PowSolver>,
    file_ids: &[String],
    target_token_id: i64,
    target_token: &str,
) -> Result<Vec<String>> {
    let mut resolved_ids = Vec::with_capacity(file_ids.len());
    for fid in file_ids {
        let resolved = resolve_file(db, client, solver, fid, target_token_id, target_token).await;
        resolved_ids.push(resolved);
    }
    Ok(resolved_ids)
}

async fn resolve_file(
    db: &Connection,
    client: &DeepSeekClient,
    solver: &Arc<PowSolver>,
    fid: &str,
    target_token_id: i64,
    target_token: &str,
) -> String {
    let Ok(Some(owner_id)) = get_file_token(db, fid).await else {
        return fid.to_string();
    };

    if owner_id == target_token_id {
        return fid.to_string();
    }

    let Ok(Some(owner_tok)) = get_token(db, owner_id).await else {
        return fid.to_string();
    };

    match rehome_single_file(
        db,
        client,
        solver,
        fid,
        &owner_tok.token,
        target_token,
        target_token_id,
    )
    .await
    {
        Ok(new_id) => new_id,
        Err(_) => fid.to_string(),
    }
}

async fn rehome_single_file(
    db: &Connection,
    client: &DeepSeekClient,
    solver: &Arc<PowSolver>,
    file_id: &str,
    src_token: &str,
    tgt_token: &str,
    tgt_token_id: i64,
) -> Result<String> {
    let bytes = client
        .download_file(src_token, file_id)
        .await
        .context("Failed downloading source file for rehome")?;

    let pow_challenge = client
        .create_pow_challenge(tgt_token, "/api/v0/file/upload_file")
        .await?;

    let pow_resp = solver.solve(&pow_challenge, "/api/v0/file/upload_file")?;

    let filename = format!("rehomed_{file_id}.bin");
    let new_file_id = client
        .upload_file(
            tgt_token,
            &pow_resp,
            &filename,
            "application/octet-stream",
            bytes,
        )
        .await
        .context("Failed re-uploading file to target account")?;

    record_file(db, &new_file_id, tgt_token_id).await?;
    Ok(new_file_id)
}
