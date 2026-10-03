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

pub fn resolve_db_path(custom: Option<&str>) -> String {
    if let Some(custom_path) = custom {
        let trimmed = custom_path.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Ok(env_path) = std::env::var("DEEPSEEKER_DB_PATH") {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let cwd_db = PathBuf::from("deeperseeker.db");
    if cwd_db.exists() {
        return "deeperseeker.db".to_string();
    }

    if let Some(data) = dirs::data_dir() {
        let app_db = data.join("deeperseeker").join("deeperseeker.db");
        return app_db.to_string_lossy().to_string();
    }

    "deeperseeker.db".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_db_path_custom_priority() {
        assert_eq!(resolve_db_path(Some("custom/path.db")), "custom/path.db");
        assert_eq!(resolve_db_path(Some("/abs/custom.db")), "/abs/custom.db");
    }

    #[test]
    fn test_resolve_db_path_empty_custom_falls_back() {
        let path = resolve_db_path(Some("   "));
        assert!(!path.is_empty());
        assert_ne!(path, "   ");
    }
}
