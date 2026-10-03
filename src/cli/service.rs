use anyhow::{Context, Result};
use std::fs::{create_dir_all, remove_file, write};
use std::path::PathBuf;
use std::process::Command;

fn get_service_path() -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("Could not determine user home directory")?;
    let service_dir = home.join(".config").join("systemd").join("user");
    let service_file = service_dir.join("deeperseeker.service");
    Ok((service_dir, service_file))
}

fn resolve_exec_path(custom_bin: Option<&str>) -> Result<PathBuf> {
    if let Some(bin) = custom_bin {
        return Ok(PathBuf::from(bin));
    }
    if let Ok(exe) = std::env::current_exe() {
        return Ok(exe);
    }
    let home = dirs::home_dir().context("Could not determine user home directory")?;
    Ok(home.join(".local").join("bin").join("deeperseeker"))
}

pub fn install_user_service(custom_bin: Option<&str>) -> Result<()> {
    let (service_dir, service_file) = get_service_path()?;
    create_dir_all(&service_dir)
        .with_context(|| format!("Failed creating directory: {:?}", service_dir))?;

    let exec_path = resolve_exec_path(custom_bin)?;
    let unit = format!(
        "[Unit]\n\
         Description=DeeperSeeker DeepSeek AI Proxy Gateway\n\
         After=network.target\n\n\
         [Service]\n\
         Type=simple\n\
         WorkingDirectory=%h\n\
         ExecStart={} serve\n\
         Restart=always\n\
         RestartSec=3s\n\
         Environment=RUST_LOG=info\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        exec_path.display()
    );

    write(&service_file, unit)
        .with_context(|| format!("Failed writing service file: {:?}", service_file))?;
    println!("[+] Created systemd user unit at {:?}", service_file);

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    println!("[+] Reloaded systemd user daemon");

    let status = Command::new("systemctl")
        .args(["--user", "enable", "--now", "deeperseeker.service"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("✔ DeeperSeeker service enabled and started successfully!");
            println!("  Inspect with: systemctl --user status deeperseeker.service");
        }
        _ => {
            println!("[!] Service installed, but automatic start returned non-zero.");
            println!("  Start manually: systemctl --user start deeperseeker.service");
        }
    }
    Ok(())
}

pub fn uninstall_user_service() -> Result<()> {
    let (_, service_file) = get_service_path()?;
    let _ = Command::new("systemctl")
        .args(["--user", "stop", "deeperseeker.service"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "deeperseeker.service"])
        .status();

    if service_file.exists() {
        remove_file(&service_file)
            .with_context(|| format!("Failed removing {:?}", service_file))?;
        println!("[-] Removed {:?}", service_file);
    }

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    println!("✔ DeeperSeeker user service uninstalled.");
    Ok(())
}

pub fn service_status() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "status", "deeperseeker.service"])
        .status()
        .context("Failed executing systemctl")?;

    if !status.success() {
        println!("[!] Service is inactive or not installed.");
    }
    Ok(())
}
