mod app;
mod client;
mod configfile;
mod kernel;
mod paths;
mod settings;
mod shell;
mod ui;

use std::io::stdout;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

const HELP: &str = "\
clash-tui —— Mihomo/Clash 终端控制器

用法：clash-tui [选项]

选项：
  --url <地址>      控制器地址（默认 http://127.0.0.1:9090）
  --secret <密钥>   控制器密钥
  --service <单元>  systemd 用户单元名（默认 mihomo-tui.service）
  --config <路径>   内核 YAML 配置路径
  --check           只检查控制器连通性并打印版本
  -h, --help        显示本帮助

环境变量：CLASH_CONTROLLER 覆盖地址，CLASH_SECRET 覆盖密钥。";

#[tokio::main]
async fn main() -> Result<()> {
    let mut settings = settings::Settings::load();
    let mut check = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--url" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    settings.url = value.clone();
                }
            }
            "--secret" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    settings.secret = value.clone();
                }
            }
            "--service" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    settings.service = value.clone();
                }
            }
            "--config" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    settings.config_path = value.clone();
                }
            }
            "--check" => check = true,
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            other => {
                eprintln!("未知参数：{other}\n\n{HELP}");
                std::process::exit(2);
            }
        }
        index += 1;
    }

    if let Ok(value) = std::env::var("CLASH_CONTROLLER") {
        if !value.is_empty() {
            settings.url = value;
        }
    }
    if let Ok(value) = std::env::var("CLASH_SECRET") {
        if !value.is_empty() {
            settings.secret = value;
        }
    }

    let client = client::Client::new(&settings.url, &settings.secret)?;

    if check {
        match client.version().await {
            Ok(version) => println!("连接成功，内核版本 {version}"),
            Err(error) => {
                eprintln!("连接失败：{error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run(&mut terminal, settings, client).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}
