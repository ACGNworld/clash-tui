use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures_util::StreamExt;
use ratatui::backend::Backend;
use ratatui::Terminal;
use tokio::sync::{mpsc, RwLock, Semaphore};

use crate::client::{Client, Configs, Connection, Provider, Proxy, Rule, Traffic};
use crate::kernel::KernelStatus;
use crate::settings::{self, Settings, Subscription};
use crate::{configfile, kernel, paths, ui};

pub type Tx = mpsc::UnboundedSender<Msg>;

/// 标签页总数（状态 / 代理 / 订阅 / 规则 / 连接 / 日志 / 设置）。
pub const TAB_COUNT: usize = 7;

#[derive(Debug)]
pub enum Msg {
    Version(Result<String>),
    Configs(Result<Configs>),
    Proxies(Result<HashMap<String, Proxy>>),
    Rules(Result<Vec<Rule>>),
    Providers(Result<HashMap<String, Provider>>),
    Connections(Result<Vec<Connection>>),
    Kernel(Result<KernelStatus>),
    Delay { target: String, result: Result<u64> },
    Traffic(Traffic),
    Log { level: String, payload: String },
    Action(Result<String>),
}

#[derive(Clone)]
pub struct Group {
    pub name: String,
    pub kind: String,
    pub now: String,
    pub nodes: Vec<String>,
}

#[derive(Clone)]
pub struct LogLine {
    pub level: String,
    pub payload: String,
}

#[derive(Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub secret: bool,
}

#[derive(Clone)]
pub struct Input {
    pub title: String,
    pub hint: String,
    pub fields: Vec<Field>,
    pub active: usize,
    pub purpose: Purpose,
}

#[derive(Clone)]
pub enum Purpose {
    AddSub,
    EditSetting(usize),
}

impl Input {
    fn value_mut(&mut self) -> &mut String {
        &mut self.fields[self.active].value
    }
}

#[derive(Clone)]
pub struct Confirm {
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Clone)]
pub enum ConfirmAction {
    DeleteSub(usize),
    CloseConnection(String),
    CloseAllConnections,
}

#[derive(Clone)]
pub enum Overlay {
    Input(Input),
    Confirm(Confirm),
    Help,
}

/// 设置页可编辑字段（序号即索引）
pub const SETTING_FIELDS: [(&str, bool); 8] = [
    ("控制器地址", false),
    ("密钥", true),
    ("systemd 单元", false),
    ("配置文件路径", false),
    ("测速地址", false),
    ("测速超时(ms)", false),
    ("测速并发数", false),
    ("内核二进制", false),
];

pub fn setting_value(settings: &Settings, index: usize) -> String {
    match index {
        0 => settings.url.clone(),
        1 => settings.secret.clone(),
        2 => settings.service.clone(),
        3 => settings.config_path.clone(),
        4 => settings.test_url.clone(),
        5 => settings.test_timeout.to_string(),
        6 => settings.workers.to_string(),
        7 => settings.binary.clone(),
        _ => String::new(),
    }
}

fn set_setting_value(settings: &mut Settings, index: usize, value: &str) {
    match index {
        0 => settings.url = value.trim().to_string(),
        1 => settings.secret = value.trim().to_string(),
        2 => settings.service = value.trim().to_string(),
        3 => settings.config_path = value.trim().to_string(),
        4 => settings.test_url = value.trim().to_string(),
        5 => settings.test_timeout = value.trim().parse().unwrap_or(settings.test_timeout),
        6 => settings.workers = value.trim().parse().unwrap_or(settings.workers),
        7 => settings.binary = value.trim().to_string(),
        _ => {}
    }
}

pub struct App {
    pub settings: Settings,
    pub subscriptions: Vec<Subscription>,
    pub client: Client,
    shared: Arc<RwLock<Client>>,

    pub tab: usize,
    pub should_quit: bool,
    pub overlay: Option<Overlay>,
    pub message: Option<(String, bool)>,
    message_age: u32,

    pub online: bool,
    pub version: Option<String>,
    pub configs: Option<Configs>,
    pub kernel: Option<KernelStatus>,
    pub kernel_error: Option<String>,
    pub traffic: Traffic,
    pub conn_count: usize,
    pub connections: Vec<Connection>,
    pub conn_index: usize,

    pub proxies: HashMap<String, Proxy>,
    pub groups: Vec<Group>,
    pub group_index: usize,
    pub node_index: usize,
    pub focus_nodes: bool,
    pub test_results: HashMap<String, u64>,
    pub testing: HashSet<String>,

    pub sub_index: usize,
    pub providers: HashMap<String, Provider>,

    pub rules: Vec<Rule>,
    pub rule_index: usize,

    pub logs: VecDeque<LogLine>,
    pub log_follow: bool,
    pub log_scroll: usize,

    pub setting_index: usize,

    tick: u64,
    pending_version: bool,
    pending_configs: bool,
    pending_proxies: bool,
    pending_rules: bool,
    pending_providers: bool,
    pending_connections: bool,
    pending_kernel: bool,
}

impl App {
    pub fn new(settings: Settings, client: Client, shared: Arc<RwLock<Client>>) -> Self {
        let subscriptions = settings::load_subscriptions();
        App {
            settings,
            subscriptions,
            client,
            shared,
            tab: 0,
            should_quit: false,
            overlay: None,
            message: None,
            message_age: 0,
            online: false,
            version: None,
            configs: None,
            kernel: None,
            kernel_error: None,
            traffic: Traffic::default(),
            conn_count: 0,
            connections: Vec::new(),
            conn_index: 0,
            proxies: HashMap::new(),
            groups: Vec::new(),
            group_index: 0,
            node_index: 0,
            focus_nodes: false,
            test_results: HashMap::new(),
            testing: HashSet::new(),
            sub_index: 0,
            providers: HashMap::new(),
            rules: Vec::new(),
            rule_index: 0,
            logs: VecDeque::new(),
            log_follow: true,
            log_scroll: 0,
            setting_index: 0,
            tick: 0,
            pending_version: false,
            pending_configs: false,
            pending_proxies: false,
            pending_rules: false,
            pending_providers: false,
            pending_connections: false,
            pending_kernel: false,
        }
    }

    pub fn set_message(&mut self, text: impl Into<String>, error: bool) {
        self.message = Some((text.into(), error));
        self.message_age = 0;
    }

    pub fn current_group(&self) -> Option<&Group> {
        self.groups.get(self.group_index)
    }

    pub fn traffic_text(&self) -> (String, String) {
        (human_rate(self.traffic.up), human_rate(self.traffic.down))
    }

    pub fn mode_label(&self) -> &'static str {
        match self.configs.as_ref().map(|c| c.mode.as_str()).unwrap_or("") {
            "rule" => "规则",
            "global" => "全局",
            "direct" => "直连",
            _ => "未知",
        }
    }

    // ---- 后台刷新 -----------------------------------------------------

    pub fn refresh_all(&mut self, tx: &Tx) {
        self.spawn_version(tx);
        self.spawn_configs(tx);
        self.spawn_proxies(tx);
        self.spawn_rules(tx);
        self.spawn_providers(tx);
        self.spawn_connections(tx);
        self.spawn_kernel(tx);
    }

    fn spawn_version(&mut self, tx: &Tx) {
        if self.pending_version {
            return;
        }
        self.pending_version = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Version(client.version().await));
        });
    }

    fn spawn_configs(&mut self, tx: &Tx) {
        if self.pending_configs {
            return;
        }
        self.pending_configs = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Configs(client.configs().await));
        });
    }

    fn spawn_proxies(&mut self, tx: &Tx) {
        if self.pending_proxies {
            return;
        }
        self.pending_proxies = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Proxies(client.proxies().await));
        });
    }

    fn spawn_rules(&mut self, tx: &Tx) {
        if self.pending_rules {
            return;
        }
        self.pending_rules = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Rules(client.rules().await));
        });
    }

    fn spawn_providers(&mut self, tx: &Tx) {
        if self.pending_providers {
            return;
        }
        self.pending_providers = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Providers(client.providers().await));
        });
    }

    fn spawn_connections(&mut self, tx: &Tx) {
        if self.pending_connections {
            return;
        }
        self.pending_connections = true;
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Connections(client.connections().await));
        });
    }

    fn spawn_kernel(&mut self, tx: &Tx) {
        if self.pending_kernel {
            return;
        }
        self.pending_kernel = true;
        let service = self.settings.service.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Msg::Kernel(kernel::status(&service).await));
        });
    }

    pub fn on_tick(&mut self, tx: &Tx) {
        self.tick += 1;
        if self.message.is_some() {
            self.message_age += 1;
            if self.message_age >= 8 {
                self.message = None;
            }
        }
        if self.tick % 2 == 0 {
            self.spawn_version(tx);
            self.spawn_configs(tx);
            self.spawn_proxies(tx);
            self.spawn_connections(tx);
        }
        if self.tick % 5 == 0 {
            self.spawn_providers(tx);
            self.spawn_rules(tx);
        }
        if self.tick % 3 == 0 {
            self.spawn_kernel(tx);
        }
    }

    // ---- 消息处理 -----------------------------------------------------

    pub fn on_msg(&mut self, msg: Msg, tx: &Tx) {
        match msg {
            Msg::Version(result) => {
                self.pending_version = false;
                match result {
                    Ok(version) => {
                        self.online = true;
                        self.version = Some(version);
                    }
                    Err(_) => {
                        self.online = false;
                        self.version = None;
                    }
                }
            }
            Msg::Configs(result) => {
                self.pending_configs = false;
                if let Ok(configs) = result {
                    self.configs = Some(configs);
                }
            }
            Msg::Proxies(result) => {
                self.pending_proxies = false;
                if let Ok(proxies) = result {
                    self.proxies = proxies;
                    self.rebuild_groups();
                }
            }
            Msg::Rules(result) => {
                self.pending_rules = false;
                if let Ok(rules) = result {
                    self.rules = rules;
                    if self.rule_index >= self.rules.len() {
                        self.rule_index = self.rules.len().saturating_sub(1);
                    }
                }
            }
            Msg::Providers(result) => {
                self.pending_providers = false;
                if let Ok(providers) = result {
                    self.providers = providers;
                }
            }
            Msg::Connections(result) => {
                self.pending_connections = false;
                if let Ok(mut connections) = result {
                    // 按下载量降序，让最活跃的连接排在最前。
                    connections.sort_by(|a, b| b.download.cmp(&a.download));
                    // 刷新会让行号变化，按 id 保住当前选中项。
                    let selected = self.connections.get(self.conn_index).map(|c| c.id.clone());
                    self.connections = connections;
                    self.conn_count = self.connections.len();
                    if let Some(id) = selected {
                        if let Some(index) = self.connections.iter().position(|c| c.id == id) {
                            self.conn_index = index;
                        }
                    }
                    if self.conn_index >= self.connections.len() {
                        self.conn_index = self.connections.len().saturating_sub(1);
                    }
                }
            }
            Msg::Kernel(result) => {
                self.pending_kernel = false;
                match result {
                    Ok(status) => {
                        self.kernel = Some(status);
                        self.kernel_error = None;
                    }
                    Err(error) => {
                        self.kernel = None;
                        self.kernel_error = Some(error.to_string());
                    }
                }
            }
            Msg::Delay { target, result } => {
                self.testing.remove(&target);
                match result {
                    Ok(delay) => {
                        self.test_results.insert(target.clone(), delay);
                    }
                    Err(_) => {
                        self.test_results.insert(target, 0);
                    }
                }
            }
            Msg::Traffic(traffic) => {
                self.traffic = traffic;
            }
            Msg::Log { level, payload } => {
                self.logs.push_back(LogLine { level, payload });
                while self.logs.len() > 2000 {
                    self.logs.pop_front();
                }
            }
            Msg::Action(result) => match result {
                Ok(text) => {
                    self.set_message(text, false);
                    self.refresh_all(tx);
                }
                Err(error) => {
                    self.set_message(error.to_string(), true);
                }
            },
        }
    }

    fn rebuild_groups(&mut self) {
        let mut groups: Vec<Group> = self
            .proxies
            .iter()
            .filter(|(_, proxy)| proxy.is_group())
            .map(|(name, proxy)| Group {
                name: name.clone(),
                kind: proxy.kind.clone(),
                now: proxy.now.clone().unwrap_or_default(),
                nodes: proxy.all.clone().unwrap_or_default(),
            })
            .collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        self.groups = groups;
        if self.group_index >= self.groups.len() {
            self.group_index = self.groups.len().saturating_sub(1);
        }
        self.clamp_node_index();
    }

    fn clamp_node_index(&mut self) {
        let len = self
            .current_group()
            .map(|g| g.nodes.len())
            .unwrap_or(0);
        if self.node_index >= len {
            self.node_index = len.saturating_sub(1);
        }
    }

    // ---- 按键 ---------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent, tx: &Tx) {
        if self.overlay.is_some() {
            self.handle_overlay_key(key, tx);
            return;
        }

        // 全局按键
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                return;
            }
            KeyCode::Tab => {
                self.tab = (self.tab + 1) % TAB_COUNT;
                return;
            }
            KeyCode::BackTab => {
                self.tab = (self.tab + TAB_COUNT - 1) % TAB_COUNT;
                return;
            }
            KeyCode::Char(c @ '1'..='9') => {
                let index = (c as usize) - ('1' as usize);
                if index < TAB_COUNT {
                    self.tab = index;
                }
                return;
            }
            KeyCode::Char('r') if key.modifiers.is_empty() => {
                self.refresh_all(tx);
                return;
            }
            KeyCode::Char('m') => {
                self.cycle_mode(tx);
                return;
            }
            KeyCode::Char('p') => {
                self.toggle_proxy(tx);
                return;
            }
            KeyCode::Char('s') => {
                self.start_kernel(tx);
                return;
            }
            KeyCode::Char('x') => {
                self.stop_kernel(tx);
                return;
            }
            KeyCode::Char('R') => {
                self.restart_kernel(tx);
                return;
            }
            _ => {}
        }

        match self.tab {
            0 => {}
            1 => self.proxies_key(key, tx),
            2 => self.subs_key(key, tx),
            3 => self.rules_key(key, tx),
            4 => self.connections_key(key, tx),
            5 => self.logs_key(key),
            6 => self.settings_key(key, tx),
            _ => {}
        }
    }

    fn proxies_key(&mut self, key: KeyEvent, tx: &Tx) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus_nodes {
                    self.node_index = self.node_index.saturating_sub(1);
                } else {
                    self.group_index = self.group_index.saturating_sub(1);
                    self.node_index = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus_nodes {
                    let len = self.current_group().map(|g| g.nodes.len()).unwrap_or(0);
                    if self.node_index + 1 < len {
                        self.node_index += 1;
                    }
                } else if self.group_index + 1 < self.groups.len() {
                    self.group_index += 1;
                    self.node_index = 0;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if self.focus_nodes {
                    self.select_current(tx);
                } else {
                    self.focus_nodes = true;
                    self.node_index = 0;
                }
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                self.focus_nodes = false;
            }
            KeyCode::Char('t') => {
                if let Some(node) = self.selected_node() {
                    self.spawn_test_node(node, tx);
                    self.set_message("测速中…", false);
                }
            }
            KeyCode::Char('T') => {
                self.spawn_test_group(tx);
                self.set_message("整组测速中…", false);
            }
            _ => {}
        }
        self.clamp_node_index();
    }

    fn subs_key(&mut self, key: KeyEvent, tx: &Tx) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.sub_index = self.sub_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.sub_index + 1 < self.subscriptions.len() {
                    self.sub_index += 1;
                }
            }
            KeyCode::Char('a') => {
                self.overlay = Some(Overlay::Input(Input {
                    title: "新增订阅".to_string(),
                    hint: "Tab 切换字段 · Enter 提交 · Esc 取消".to_string(),
                    fields: vec![
                        Field {
                            label: "名称".to_string(),
                            value: String::new(),
                            secret: false,
                        },
                        Field {
                            label: "链接".to_string(),
                            value: String::new(),
                            secret: false,
                        },
                    ],
                    active: 0,
                    purpose: Purpose::AddSub,
                }));
            }
            KeyCode::Char('d') => {
                if let Some(sub) = self.subscriptions.get(self.sub_index) {
                    self.overlay = Some(Overlay::Confirm(Confirm {
                        message: format!("确定删除订阅「{}」吗？", sub.name),
                        action: ConfirmAction::DeleteSub(self.sub_index),
                    }));
                }
            }
            KeyCode::Char('u') => {
                if let Some(sub) = self.subscriptions.get(self.sub_index) {
                    let name = sub.name.clone();
                    let client = self.client.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let result = client
                            .update_provider(&name)
                            .await
                            .map(|_| format!("订阅 {name} 已更新"));
                        let _ = tx.send(Msg::Action(result));
                    });
                }
            }
            KeyCode::Char('h') => {
                if let Some(sub) = self.subscriptions.get(self.sub_index) {
                    let name = sub.name.clone();
                    let client = self.client.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let result = client
                            .provider_healthcheck(&name)
                            .await
                            .map(|_| format!("订阅 {name} 健康检查完成"));
                        let _ = tx.send(Msg::Action(result));
                    });
                }
            }
            KeyCode::Char('w') => {
                self.apply_subscriptions(tx);
            }
            _ => {}
        }
    }

    fn rules_key(&mut self, key: KeyEvent, _tx: &Tx) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.rule_index = self.rule_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.rule_index + 1 < self.rules.len() {
                    self.rule_index += 1;
                }
            }
            _ => {}
        }
    }

    fn connections_key(&mut self, key: KeyEvent, tx: &Tx) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.conn_index = self.conn_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.conn_index + 1 < self.connections.len() {
                    self.conn_index += 1;
                }
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.conn_index = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.connections.is_empty() {
                    self.conn_index = self.connections.len() - 1;
                }
            }
            KeyCode::Enter | KeyCode::Char('d') => {
                if let Some(conn) = self.connections.get(self.conn_index) {
                    self.overlay = Some(Overlay::Confirm(Confirm {
                        message: format!("关闭连接「{}」吗？", connection_target(conn)),
                        action: ConfirmAction::CloseConnection(conn.id.clone()),
                    }));
                }
            }
            KeyCode::Char('D') => {
                if !self.connections.is_empty() {
                    self.overlay = Some(Overlay::Confirm(Confirm {
                        message: format!("关闭全部 {} 条连接吗？", self.connections.len()),
                        action: ConfirmAction::CloseAllConnections,
                    }));
                } else {
                    self.set_message("当前没有活动连接", true);
                }
            }
            _ => {
                let _ = tx;
            }
        }
    }

    fn logs_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.log_follow = false;
                self.log_scroll = self.log_scroll.saturating_add(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
            KeyCode::Char('f') => {
                self.log_follow = !self.log_follow;
                self.log_scroll = 0;
            }
            KeyCode::Char('c') => {
                self.logs.clear();
                self.log_scroll = 0;
            }
            _ => {}
        }
    }

    fn settings_key(&mut self, key: KeyEvent, tx: &Tx) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.setting_index = self.setting_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.setting_index + 1 < SETTING_FIELDS.len() {
                    self.setting_index += 1;
                }
            }
            KeyCode::Enter => {
                let (label, secret) = SETTING_FIELDS[self.setting_index];
                self.overlay = Some(Overlay::Input(Input {
                    title: format!("修改：{label}"),
                    hint: "Enter 保存 · Esc 取消".to_string(),
                    fields: vec![Field {
                        label: label.to_string(),
                        value: setting_value(&self.settings, self.setting_index),
                        secret,
                    }],
                    active: 0,
                    purpose: Purpose::EditSetting(self.setting_index),
                }));
            }
            KeyCode::Char('w') => match self.settings.save() {
                Ok(()) => {
                    self.set_message("设置已保存", false);
                    let _ = tx;
                }
                Err(error) => self.set_message(format!("保存失败：{error}"), true),
            },
            _ => {}
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, tx: &Tx) {
        let overlay = self.overlay.clone();
        match overlay {
            Some(Overlay::Help) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                    self.overlay = None;
                }
            }
            Some(Overlay::Confirm(confirm)) => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.overlay = None;
                    match confirm.action {
                        ConfirmAction::DeleteSub(index) => {
                            if index < self.subscriptions.len() {
                                self.subscriptions.remove(index);
                                if self.sub_index >= self.subscriptions.len() {
                                    self.sub_index = self.subscriptions.len().saturating_sub(1);
                                }
                                if let Err(error) = settings::save_subscriptions(&self.subscriptions) {
                                    self.set_message(format!("保存失败：{error}"), true);
                                } else {
                                    self.apply_subscriptions(tx);
                                }
                            }
                        }
                        ConfirmAction::CloseConnection(id) => {
                            let client = self.client.clone();
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                let result = client
                                    .close_connection(&id)
                                    .await
                                    .map(|_| "已关闭连接".to_string());
                                let _ = tx.send(Msg::Action(result));
                            });
                        }
                        ConfirmAction::CloseAllConnections => {
                            let client = self.client.clone();
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                let result = client
                                    .close_all_connections()
                                    .await
                                    .map(|_| "已关闭全部连接".to_string());
                                let _ = tx.send(Msg::Action(result));
                            });
                        }
                    }
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.overlay = None;
                }
                _ => {}
            },
            Some(Overlay::Input(mut input)) => match key.code {
                KeyCode::Esc => {
                    self.overlay = None;
                }
                KeyCode::Enter => {
                    if input.active + 1 < input.fields.len() {
                        input.active += 1;
                        self.overlay = Some(Overlay::Input(input));
                    } else {
                        self.overlay = None;
                        self.submit_input(input, tx);
                    }
                }
                KeyCode::Tab => {
                    input.active = (input.active + 1) % input.fields.len();
                    self.overlay = Some(Overlay::Input(input));
                }
                KeyCode::Backspace => {
                    input.value_mut().pop();
                    self.overlay = Some(Overlay::Input(input));
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.value_mut().clear();
                    self.overlay = Some(Overlay::Input(input));
                }
                KeyCode::Char(c) => {
                    input.value_mut().push(c);
                    self.overlay = Some(Overlay::Input(input));
                }
                _ => {
                    self.overlay = Some(Overlay::Input(input));
                }
            },
            None => {}
        }
    }

    fn submit_input(&mut self, input: Input, tx: &Tx) {
        match input.purpose {
            Purpose::AddSub => {
                let name = input.fields[0].value.trim().to_string();
                let url = input.fields[1].value.trim().to_string();
                let sub = Subscription {
                    name: name.clone(),
                    url,
                    interval: 36000,
                    health_interval: 600,
                    group: String::new(),
                    health_url: crate::client::DEFAULT_TEST_URL.to_string(),
                };
                let mut candidate = self.subscriptions.clone();
                candidate.push(sub.clone());
                if let Err(error) = configfile::validate(&candidate) {
                    self.set_message(error.to_string(), true);
                    return;
                }
                self.subscriptions = candidate;
                if let Err(error) = settings::save_subscriptions(&self.subscriptions) {
                    self.set_message(format!("保存失败：{error}"), true);
                    return;
                }
                self.set_message(format!("已添加订阅 {name}"), false);
                self.apply_subscriptions(tx);
            }
            Purpose::EditSetting(index) => {
                let value = input.fields[0].value.clone();
                set_setting_value(&mut self.settings, index, &value);
                if let Err(error) = self.settings.save() {
                    self.set_message(format!("保存失败：{error}"), true);
                    return;
                }
                self.sync_client();
                self.set_message("设置已保存", false);
                self.refresh_all(tx);
            }
        }
    }

    fn sync_client(&mut self) {
        match Client::new(&self.settings.url, &self.settings.secret) {
            Ok(client) => {
                self.client = client.clone();
                let shared = self.shared.clone();
                tokio::spawn(async move {
                    *shared.write().await = client;
                });
            }
            Err(error) => self.set_message(error.to_string(), true),
        }
    }

    // ---- 操作 ---------------------------------------------------------

    fn selected_node(&self) -> Option<String> {
        if self.focus_nodes {
            self.current_group()
                .and_then(|g| g.nodes.get(self.node_index).cloned())
        } else {
            self.current_group().map(|g| g.now.clone())
        }
    }

    fn spawn_test_node(&mut self, node: String, tx: &Tx) {
        if node.is_empty() {
            return;
        }
        self.testing.insert(node.clone());
        let client = self.client.clone();
        let url = self.settings.test_url.clone();
        let timeout = self.settings.test_timeout;
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = client.delay(&node, &url, timeout).await;
            let _ = tx.send(Msg::Delay {
                target: node,
                result,
            });
        });
    }

    fn spawn_test_group(&mut self, tx: &Tx) {
        let Some(group) = self.current_group().cloned() else {
            return;
        };
        if group.nodes.is_empty() {
            self.set_message("该组没有可测速的节点", true);
            return;
        }
        for node in &group.nodes {
            self.testing.insert(node.clone());
        }
        let client = self.client.clone();
        let url = self.settings.test_url.clone();
        let timeout = self.settings.test_timeout;
        let workers = self.settings.workers.max(1);
        let tx = tx.clone();
        tokio::spawn(async move {
            let semaphore = Arc::new(Semaphore::new(workers));
            for node in group.nodes.clone() {
                let semaphore = semaphore.clone();
                let client = client.clone();
                let url = url.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await;
                    let result = client.delay(&node, &url, timeout).await;
                    let _ = tx.send(Msg::Delay {
                        target: node,
                        result,
                    });
                });
            }
        });
    }

    fn select_current(&mut self, tx: &Tx) {
        let Some(group) = self.current_group().cloned() else {
            return;
        };
        if group.kind != "Selector" {
            self.set_message(format!("{} 由内核自动管理，无法手动选择", group.name), true);
            return;
        }
        let Some(node) = group.nodes.get(self.node_index).cloned() else {
            return;
        };
        let client = self.client.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = client
                .select(&group.name, &node)
                .await
                .map(|_| format!("已选择节点 {node}"));
            let _ = tx.send(Msg::Action(result));
        });
    }

    fn set_mode(&mut self, mode: &str, tx: &Tx) {
        let client = self.client.clone();
        let tx = tx.clone();
        let label = match mode {
            "rule" => "规则模式".to_string(),
            "global" => "全局模式".to_string(),
            "direct" => "直连模式".to_string(),
            _ => mode.to_string(),
        };
        let mode = mode.to_string();
        tokio::spawn(async move {
            let result = client
                .set_mode(&mode)
                .await
                .map(|_| format!("已切换到{label}"));
            let _ = tx.send(Msg::Action(result));
        });
    }

    fn cycle_mode(&mut self, tx: &Tx) {
        let current = self
            .configs
            .as_ref()
            .map(|c| c.mode.as_str())
            .unwrap_or("rule");
        let next = match current {
            "rule" => "global",
            "global" => "direct",
            _ => "rule",
        };
        self.set_mode(next, tx);
    }

    fn toggle_proxy(&mut self, tx: &Tx) {
        let current = self
            .configs
            .as_ref()
            .map(|c| c.mode.as_str())
            .unwrap_or("rule");
        let next = if current == "direct" { "rule" } else { "direct" };
        self.set_mode(next, tx);
    }

    fn start_kernel(&mut self, tx: &Tx) {
        let service = self.settings.service.clone();
        let binary = self.settings.binary.clone();
        let config_path = self.settings.config_path.clone();
        let log = paths::logs_dir().join("mihomo.log");
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = match kernel::control(&service, "start").await {
                Ok(_) => Ok("内核已启动".to_string()),
                Err(error) if error.to_string().contains("没有可用的 systemd 用户会话") => {
                    let config_dir = Path::new(&config_path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    kernel::detached_start(&binary, &config_dir, &config_path, &log)
                        .await
                        .map(|_| "内核已在后台启动（无 systemd）".to_string())
                }
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Action(result));
        });
    }

    fn stop_kernel(&mut self, tx: &Tx) {
        let service = self.settings.service.clone();
        let config_path = self.settings.config_path.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = match kernel::control(&service, "stop").await {
                Ok(_) => Ok("内核已停止".to_string()),
                Err(error) if error.to_string().contains("没有可用的 systemd 用户会话") => {
                    let config_dir = Path::new(&config_path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    kernel::detached_stop(&config_dir)
                        .await
                        .map(|_| "后台内核已停止".to_string())
                }
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Action(result));
        });
    }

    fn restart_kernel(&mut self, tx: &Tx) {
        let service = self.settings.service.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = kernel::control(&service, "restart")
                .await
                .map(|_| "内核已重启".to_string());
            let _ = tx.send(Msg::Action(result));
        });
    }

    fn apply_subscriptions(&mut self, tx: &Tx) {
        let subs = self.subscriptions.clone();
        let path = self.settings.config_path.clone();
        let rule_target = if self.settings.manage_rules {
            if !self.settings.rules_group.is_empty() {
                Some(self.settings.rules_group.clone())
            } else {
                self.subscriptions.first().map(|s| s.group_name())
            }
        } else {
            None
        };
        let client = if self.online {
            Some(self.client.clone())
        } else {
            None
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            match configfile::apply(&path, &subs, rule_target.as_deref(), client.as_ref()).await {
                Ok(action) => {
                    let note = if client.is_none() {
                        format!("托管块已{}（内核未运行，重启后生效）", action.label())
                    } else {
                        format!("托管块已{}并已重载内核", action.label())
                    };
                    let _ = tx.send(Msg::Action(Ok(note)));
                }
                Err(error) => {
                    let _ = tx.send(Msg::Action(Err(error)));
                }
            }
        });
    }
}

/// 连接的展示目标：优先域名，其次目标 IP，并带上端口。
pub fn connection_target(conn: &Connection) -> String {
    let host = if !conn.metadata.host.is_empty() {
        conn.metadata.host.clone()
    } else if !conn.metadata.destination_ip.is_empty() {
        conn.metadata.destination_ip.clone()
    } else {
        "-".to_string()
    };
    if conn.metadata.destination_port.is_empty() {
        host
    } else {
        format!("{host}:{}", conn.metadata.destination_port)
    }
}

fn human_rate(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub async fn run_log_stream(shared: Arc<RwLock<Client>>, tx: Tx) {
    loop {
        let client = shared.read().await.clone();
        if let Ok(response) = client.stream("/logs?level=info").await {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(position) = buffer.find('\n') {
                    let line: String = buffer.drain(..=position).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                        let level = value
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("info")
                            .to_string();
                        let payload = value
                            .get("payload")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let _ = tx.send(Msg::Log { level, payload });
                    }
                }
                if buffer.len() > 1 << 20 {
                    buffer.clear();
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub async fn run_traffic_stream(shared: Arc<RwLock<Client>>, tx: Tx) {
    loop {
        let client = shared.read().await.clone();
        if let Ok(response) = client.stream("/traffic").await {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(position) = buffer.find('\n') {
                    let line: String = buffer.drain(..=position).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(traffic) = serde_json::from_str::<Traffic>(line) {
                        let _ = tx.send(Msg::Traffic(traffic));
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

pub async fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    settings: Settings,
    client: Client,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let shared = Arc::new(RwLock::new(client.clone()));
    let mut app = App::new(settings, client, shared.clone());
    app.refresh_all(&tx);
    tokio::spawn(run_log_stream(shared.clone(), tx.clone()));
    tokio::spawn(run_traffic_stream(shared.clone(), tx.clone()));

    let mut events = crossterm::event::EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        tokio::select! {
            maybe = events.next() => {
                match maybe {
                    Some(Ok(crossterm::event::Event::Key(key)))
                        if key.kind == crossterm::event::KeyEventKind::Press =>
                    {
                        app.on_key(key, &tx);
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => {
                        app.should_quit = true;
                    }
                }
            }
            Some(msg) = rx.recv() => {
                app.on_msg(msg, &tx);
            }
            _ = ticker.tick() => {
                app.on_tick(&tx);
            }
        }
    }
    Ok(())
}
