use std::path::PathBuf;

/// `$XDG_CONFIG_HOME` 或 `~/.config`
pub fn config_home() -> PathBuf {
    if let Ok(value) = std::env::var("XDG_CONFIG_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config")
}

/// 本工具自身的状态目录：`~/.config/clash-tui`
pub fn tui_dir() -> PathBuf {
    config_home().join("clash-tui")
}

pub fn settings_path() -> PathBuf {
    tui_dir().join("settings.json")
}

pub fn subscriptions_path() -> PathBuf {
    tui_dir().join("subscriptions.json")
}

pub fn logs_dir() -> PathBuf {
    tui_dir().join("logs")
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}
