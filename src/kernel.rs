//! 内核生命周期管理。
//!
//! 优先使用 systemd 用户服务（`systemctl --user`），这样 TUI 退出后内核仍然运行；
//! 当没有 systemd 用户会话时，退回到 `setsid` 脱离终端启动。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

#[derive(Clone, Debug, Default)]
pub struct KernelStatus {
    pub loaded: String,
    pub active: bool,
    pub state: String,
    pub sub: String,
    pub since: String,
}

fn no_session(stderr: &str) -> bool {
    stderr.contains("Failed to connect to bus") || stderr.contains("Cannot autolaunch")
}

pub async fn status(unit: &str) -> Result<KernelStatus> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            unit,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "SubState",
            "-p",
            "ActiveEnterTimestamp",
        ])
        .output()
        .await
        .context("无法执行 systemctl")?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        if no_session(&stderr) {
            bail!("没有可用的 systemd 用户会话");
        }
        bail!("读取单元状态失败：{stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut values: HashMap<String, String> = HashMap::new();
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    if values.get("LoadState").map(String::as_str) == Some("not-found") {
        bail!("未找到 systemd 用户单元 {unit}");
    }
    let state = values.get("ActiveState").cloned().unwrap_or_default();
    Ok(KernelStatus {
        loaded: values.get("LoadState").cloned().unwrap_or_default(),
        active: state == "active",
        state,
        sub: values.get("SubState").cloned().unwrap_or_default(),
        since: values.get("ActiveEnterTimestamp").cloned().unwrap_or_default(),
    })
}

pub async fn control(unit: &str, action: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .args(["--user", action, unit])
        .output()
        .await
        .context("无法执行 systemctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if no_session(&stderr) {
            bail!("没有可用的 systemd 用户会话");
        }
        let detail = if stderr.is_empty() {
            format!("退出码 {}", output.status)
        } else {
            stderr
        };
        bail!("systemctl {action} {unit} 失败：{detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn journal(unit: &str, lines: usize) -> Result<String> {
    let output = Command::new("journalctl")
        .args([
            "--user",
            "-u",
            unit,
            "-n",
            &lines.to_string(),
            "--no-pager",
        ])
        .output()
        .await
        .context("无法执行 journalctl")?;
    if !output.status.success() {
        bail!(
            "journalctl 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// 无 systemd 时的兜底：脱离终端启动内核，TUI 退出后仍运行。
pub async fn detached_start(binary: &str, config_dir: &str, config_file: &str, log: &Path) -> Result<()> {
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .with_context(|| format!("无法打开日志文件 {}", log.display()))?;
    let stderr = stdout.try_clone()?;
    let status = Command::new("setsid")
        .arg("-f")
        .arg(binary)
        .args(["-d", config_dir, "-f", config_file])
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .await
        .context("无法执行 setsid")?;
    if !status.success() {
        bail!("脱离终端启动内核失败");
    }
    Ok(())
}

pub async fn detached_stop(config_dir: &str) -> Result<()> {
    let pattern = format!("mihomo -d {config_dir}");
    let output = Command::new("pkill")
        .args(["-f", &pattern])
        .output()
        .await
        .context("无法执行 pkill")?;
    if !output.status.success() {
        bail!("没有找到匹配 {pattern} 的内核进程");
    }
    Ok(())
}
