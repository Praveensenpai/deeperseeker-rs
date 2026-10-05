use anyhow::{anyhow, bail, Context, Result};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

const REPO: &str = "Praveensenpai/deeperseeker-rs";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run_update(yes: bool) -> Result<()> {
    let arch = detect_arch()?;
    println!("Current version : v{CURRENT_VERSION}");
    println!("Architecture    : {arch}");

    let client = build_client()?;
    let (tag, download_url) = fetch_latest_release(&client, arch).await?;

    let latest = tag.trim_start_matches('v');
    if latest == CURRENT_VERSION {
        println!("Already up to date (v{CURRENT_VERSION}).");
        return Ok(());
    }

    println!("New version available: {tag}");

    if !yes && !confirm_prompt()? {
        println!("Update cancelled.");
        return Ok(());
    }

    let bin_path = current_bin_path()?;
    println!("Downloading {download_url} ...");
    let archive = download_bytes(&client, &download_url).await?;

    let new_bin = extract_binary_from_tarball(&archive)?;
    replace_binary(&bin_path, &new_bin)?;

    println!("Updated to {tag} — restart the process (or service) to apply.");
    maybe_restart_service();
    Ok(())
}

fn detect_arch() -> Result<&'static str> {
    match env::consts::ARCH {
        "x86_64" => Ok("x86_64"),
        "aarch64" => Ok("aarch64"),
        other => bail!("Unsupported architecture for auto-update: {other}"),
    }
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("deeperseeker-updater/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed building HTTP client")
}

async fn fetch_latest_release(client: &reqwest::Client, arch: &str) -> Result<(String, String)> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = client
        .get(&url)
        .send()
        .await
        .context("GitHub API request failed")?
        .error_for_status()
        .context("GitHub API returned error status")?;

    let json: serde_json::Value = resp
        .json()
        .await
        .context("Failed parsing GitHub API JSON")?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing tag_name in GitHub release"))?
        .to_string();

    let asset_name = format!("deeperseeker-{arch}-linux.tar.gz");
    let download_url = json["assets"]
        .as_array()
        .ok_or_else(|| anyhow!("No assets in GitHub release"))?
        .iter()
        .find(|a| a["name"].as_str() == Some(&asset_name))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| anyhow!("Asset {asset_name} not found in release {tag}"))?
        .to_string();

    Ok((tag, download_url))
}

async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("Download request failed")?
        .error_for_status()
        .context("Download returned error status")?;

    let bytes = resp.bytes().await.context("Failed reading download body")?;
    Ok(bytes.to_vec())
}

fn extract_binary_from_tarball(archive: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(archive);
    let mut tar = Archive::new(gz);

    for entry in tar.entries().context("Failed reading tar entries")? {
        let mut entry = entry.context("Corrupt tar entry")?;
        let path = entry.path().context("Invalid tar entry path")?;
        if path.file_name().and_then(|n| n.to_str()) == Some("deeperseeker") {
            let mut buf = Vec::new();
            io::copy(&mut entry, &mut buf).context("Failed extracting binary from archive")?;
            return Ok(buf);
        }
    }
    bail!("Binary 'deeperseeker' not found inside archive")
}

fn replace_binary(bin_path: &PathBuf, new_bin: &[u8]) -> Result<()> {
    let tmp = bin_path.with_extension("tmp");
    fs::write(&tmp, new_bin).context("Failed writing temporary binary")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&tmp, perms).context("Failed setting execute permissions")?;
    }

    fs::rename(&tmp, bin_path).context("Failed replacing binary (rename)")?;
    println!("Binary replaced: {}", bin_path.display());
    Ok(())
}

fn current_bin_path() -> Result<PathBuf> {
    env::current_exe().context("Failed resolving current binary path")
}

fn confirm_prompt() -> Result<bool> {
    print!("Apply update? [y/N] ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn maybe_restart_service() {
    let status = std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "deeperseeker"])
        .status();
    if status.map(|s| s.success()).unwrap_or(false) {
        println!("Restarting systemd user service...");
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "restart", "deeperseeker"])
            .status();
    }
}
