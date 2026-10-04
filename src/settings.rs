use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

fn default_url() -> String {
    "http://127.0.0.1:9090".to_string()
}
fn default_service() -> String {
    "mihomo-tui.service".to_string()
}
fn default_test_url() -> String {
    "https://www.gstatic.com/generate_204".to_string()
}
fn default_test_timeout() -> u64 {
    3000
}
fn default_workers() -> usize {
    8
}
fn default_true() -> bool {
    true
}
fn default_env_port() -> u16 {
    7890
}
fn default_binary() -> String {
    "/usr/bin/mihomo".to_string()
}
fn default_config_path() -> String {
    paths::config_home()
        .join("mihomo")
        .join("config.yaml")
        .to_string_lossy()
        .to_string()
}
fn default_interval() -> u64 {
    36000
}
fn default_health_interval() -> u64 {
    600
}

/// 与旧版 Python 工具保持兼容的字段集合。
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_url")]
    pub url: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default = "default_service")]
    pub service: String,
    #[serde(default = "default_config_path")]
    pub config_path: String,
    #[serde(default = "default_test_url")]
    pub test_url: String,
    #[serde(default = "default_test_timeout")]
    pub test_timeout: u64,
    #[serde(default = "default_workers")]
    pub workers: usize,
    #[serde(default = "default_true")]
    pub manage_rules: bool,
    #[serde(default)]
    pub rules_group: String,
    #[serde(default = "default_true")]
    pub sort_by_delay: bool,
    #[serde(default = "default_env_port")]
    pub env_port: u16,
    #[serde(default = "default_binary")]
    pub binary: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            url: default_url(),
            secret: String::new(),
            service: default_service(),
            config_path: default_config_path(),
            test_url: default_test_url(),
            test_timeout: default_test_timeout(),
            workers: default_workers(),
            manage_rules: true,
            rules_group: String::new(),
            sort_by_delay: true,
            env_port: default_env_port(),
            binary: default_binary(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = paths::settings_path();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        write_private(&path, &text)?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub name: String,
    pub url: String,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_health_interval")]
    pub health_interval: u64,
    #[serde(default)]
    pub group: String,
    #[serde(default = "default_test_url")]
    pub health_url: String,
}

impl Subscription {
    pub fn group_name(&self) -> String {
        if self.group.is_empty() {
            format!("sub-{}", self.name)
        } else {
            self.group.clone()
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct SubsFile {
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
}

pub fn load_subscriptions() -> Vec<Subscription> {
    let path = paths::subscriptions_path();
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<SubsFile>(&text)
            .map(|f| f.subscriptions)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_subscriptions(items: &[Subscription]) -> Result<()> {
    let path = paths::subscriptions_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&SubsFile {
        subscriptions: items.to_vec(),
    })?;
    write_private(&path, &text)?;
    Ok(())
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).with_context(|| format!("无法写入 {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).ok();
    Ok(())
}
