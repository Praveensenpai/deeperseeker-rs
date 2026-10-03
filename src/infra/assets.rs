use std::path::{Path, PathBuf};

pub fn resolve_asset_dir(relative: &str) -> PathBuf {
    let cwd_path = PathBuf::from(relative);
    if cwd_path.exists() {
        return cwd_path;
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let exe_path = parent.join(relative);
            if exe_path.exists() {
                return exe_path;
            }
        }
    }

    if let Some(data) = dirs::data_dir() {
        let app_path = data.join("deeperseeker").join(relative);
        if app_path.exists() {
            return app_path;
        }
    }

    cwd_path
}

pub fn resolve_templates_pattern() -> String {
    let dir = resolve_asset_dir("templates");
    format!("{}/**/*", dir.to_string_lossy())
}

pub fn resolve_wasm_path(default_rel: &str) -> String {
    if Path::new(default_rel).is_absolute() && Path::new(default_rel).exists() {
        return default_rel.to_string();
    }
    let p = resolve_asset_dir(default_rel);
    p.to_string_lossy().to_string()
}
