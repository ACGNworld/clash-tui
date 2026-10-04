use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Sparkline, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{connection_target, setting_value, App, Overlay, SETTING_FIELDS};

const TAB_TITLES: [&str; 7] = [
    "1 状态", "2 代理", "3 订阅", "4 规则", "5 连接", "6 日志", "7 设置",
];

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(frame, app, chunks[0]);
    draw_tabs(frame, app, chunks[1]);
    match app.tab {
        0 => draw_status(frame, app, chunks[2]),
        1 => draw_proxies(frame, app, chunks[2]),
        2 => draw_subs(frame, app, chunks[2]),
        3 => draw_rules(frame, app, chunks[2]),
        4 => draw_connections(frame, app, chunks[2]),
        5 => draw_logs(frame, app, chunks[2]),
        6 => draw_settings(frame, app, chunks[2]),
        _ => {}
    }
    draw_footer(frame, app, chunks[3]);
    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, overlay, area);
    }
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "Selector" => "手动选择",
        "URLTest" => "自动测速",
        "Fallback" => "故障转移",
        "LoadBalance" => "负载均衡",
        "Relay" => "链式代理",
        "Direct" => "直连",
        "Reject" => "拒绝",
        "Compatible" | "Pass" => "兼容",
        other => other,
    }
}

fn current_node(app: &App) -> String {
    app.groups
        .iter()
        .find(|g| g.name == "GLOBAL")
        .or_else(|| app.groups.iter().find(|g| !g.nodes.is_empty()))
        .map(|g| {
            if g.now.is_empty() {
                "-".to_string()
            } else {
                g.now.clone()
            }
        })
        .unwrap_or_else(|| "-".to_string())
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (kernel_text, kernel_color) = match (&app.kernel, &app.kernel_error) {
        (Some(status), _) if status.active => ("内核运行中".to_string(), Color::Green),
        (Some(_), _) => ("内核已停止".to_string(), Color::Red),
        (None, Some(_)) => ("内核状态未知".to_string(), Color::Yellow),
        (None, None) => ("内核状态…".to_string(), Color::DarkGray),
    };
    let version = app.version.clone().unwrap_or_else(|| "未连接".to_string());
    let proxy_state = if app.configs.as_ref().map(|c| c.mode.as_str()).unwrap_or("") == "direct" {
        ("代理已关闭", Color::Red)
    } else {
        ("代理已开启", Color::Green)
    };

    let line = Line::from(vec![
        Span::styled(
            " clash-tui ",
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(kernel_text, Style::default().fg(kernel_color)),
        Span::raw("  "),
        Span::styled(version, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            format!("模式 {}", app.mode_label()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(proxy_state.0, Style::default().fg(proxy_state.1)),
        Span::raw("  "),
        Span::styled(
            format!("节点 {}", current_node(app)),
            Style::default().fg(Color::Magenta),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let tabs = Tabs::new(TAB_TITLES)
        .select(app.tab)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider("│");
    frame.render_widget(tabs, area);
}

fn block(title: &str) -> Block<'_> {
    Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(Color::Cyan),
        ))
        .border_style(Style::default().fg(Color::DarkGray))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Min(8), Constraint::Length(5)]).split(area);
    let (up, down) = app.traffic_text();
    let kernel_line = match &app.kernel {
        Some(status) => format!(
            "运行状态：{}  单元：{}  加载 {}  状态 {}/{}  自 {}",
            if status.active {
                "运行中"
            } else {
                "已停止"
            },
            app.settings.service,
            status.loaded,
            status.state,
            status.sub,
            if status.since.is_empty() {
                "-".to_string()
            } else {
                status.since.clone()
            }
        ),
        None => format!(
            "运行状态：未知  {}",
            app.kernel_error.clone().unwrap_or_default()
        ),
    };
    let controller_line = format!(
        "控制器：{}  {}",
        app.settings.url,
        if app.online {
            format!("已连接（{}）", app.version.clone().unwrap_or_default())
        } else {
            "未连接".to_string()
        }
    );
    let config_line = match &app.configs {
        Some(configs) => format!(
            "内核配置：端口 {}（HTTP {} / SOCKS {}）  允许局域网 {}  IPv6 {}  日志级别 {}",
            configs.mixed_port,
            configs.port,
            configs.socks_port,
            yesno(configs.allow_lan),
            yesno(configs.ipv6),
            configs.log_level
        ),
        None => "内核配置：-".to_string(),
    };
    let traffic_line = format!(
        "实时速率：↑ {up}    ↓ {down}    累计：↑ {}    ↓ {}",
        human_total(app.traffic.up_total),
        human_total(app.traffic.down_total)
    );
    let conn_line = format!(
        "活动连接：{}    订阅数：{}",
        app.conn_count,
        app.subscriptions.len()
    );

    let lines = vec![
        Line::from(kernel_line),
        Line::from(controller_line),
        Line::from(config_line),
        Line::from(""),
        Line::from(vec![
            Span::styled("当前模式：", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.mode_label(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                if app.configs.as_ref().map(|c| c.mode.as_str()).unwrap_or("") == "direct" {
                    "（代理已关闭，全部直连）"
                } else {
                    "（代理已开启）"
                },
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("当前节点：", Style::default().fg(Color::DarkGray)),
            Span::styled(current_node(app), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(""),
        Line::from(traffic_line),
        Line::from(conn_line),
        Line::from(""),
        Line::from(Span::styled(
            "按键：s 启动内核 · x 停止内核 · R 重启 · m 切换模式 · p 开关代理 · r 刷新",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(block("状态"))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    let upload: Vec<u64> = app.traffic_history.iter().map(|sample| sample.up).collect();
    let download: Vec<u64> = app
        .traffic_history
        .iter()
        .map(|sample| sample.down)
        .collect();
    let charts = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    frame.render_widget(
        Sparkline::default()
            .block(block("上传速率（最近 60 秒）"))
            .data(&upload)
            .style(Style::default().fg(Color::Green)),
        charts[0],
    );
    frame.render_widget(
        Sparkline::default()
            .block(block("下载速率（最近 60 秒）"))
            .data(&download)
            .style(Style::default().fg(Color::Cyan)),
        charts[1],
    );
}

fn yesno(value: bool) -> &'static str {
    if value {
        "开"
    } else {
        "关"
    }
}

fn human_total(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1}{}", UNITS[unit])
}

fn draw_proxies(frame: &mut Frame, app: &App, area: Rect) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)]).split(area);

    // 左侧：代理组
    let group_items: Vec<ListItem> = app
        .groups
        .iter()
        .map(|group| {
            let line = Line::from(vec![
                Span::raw(group.name.clone()),
                Span::raw("  "),
                Span::styled(
                    format!("[{}]", kind_label(&group.kind)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();
    let mut group_state = ListState::default();
    if !app.groups.is_empty() {
        group_state.select(Some(app.group_index));
    }
    let group_title = format!("代理组（{}）", app.groups.len());
    let group_list = List::new(group_items)
        .block(block(&group_title))
        .highlight_style(if app.focus_nodes {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        })
        .highlight_symbol(if app.focus_nodes { "  " } else { "▶ " });
    frame.render_stateful_widget(group_list, chunks[0], &mut group_state);

    // 右侧：节点
    let mut node_items: Vec<ListItem> = Vec::new();
    if let Some(group) = app.current_group() {
        for node in &group.nodes {
            let is_current = *node == group.now;
            let proxy_kind = app
                .proxies
                .get(node)
                .map(|p| {
                    if p.udp {
                        format!("{} UDP", kind_label(&p.kind))
                    } else {
                        kind_label(&p.kind).to_string()
                    }
                })
                .unwrap_or_default();
            let delay = render_delay(app, node);
            let marker = if is_current { "● " } else { "  " };
            let mut spans = vec![
                Span::styled(
                    marker,
                    Style::default().fg(if is_current {
                        Color::Green
                    } else {
                        Color::DarkGray
                    }),
                ),
                Span::styled(
                    node.clone(),
                    if is_current {
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::raw("  "),
                Span::styled(proxy_kind, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
            ];
            spans.push(delay);
            node_items.push(ListItem::new(Line::from(spans)));
        }
    }
    let mut node_state = ListState::default();
    if !node_items.is_empty() {
        node_state.select(Some(app.node_index));
    }
    let title = app
        .current_group()
        .map(|g| {
            format!(
                "节点 · {} · 当前 {}",
                g.name,
                if g.now.is_empty() { "-" } else { &g.now }
            )
        })
        .unwrap_or_else(|| "节点".to_string());
    let node_list = List::new(node_items)
        .block(block(&title))
        .highlight_style(if app.focus_nodes {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        })
        .highlight_symbol(if app.focus_nodes { "▶ " } else { "  " });
    frame.render_stateful_widget(node_list, chunks[1], &mut node_state);
}

fn render_delay(app: &App, node: &str) -> Span<'static> {
    if app.testing.contains(node) {
        return Span::styled("测速中…", Style::default().fg(Color::Yellow));
    }
    let value = app
        .test_results
        .get(node)
        .copied()
        .or_else(|| app.proxies.get(node).and_then(|p| p.last_delay()));
    match value {
        Some(0) => Span::styled("超时", Style::default().fg(Color::Red)),
        Some(delay) => {
            let color = if delay < 200 {
                Color::Green
            } else if delay < 500 {
                Color::Yellow
            } else {
                Color::Red
            };
            Span::styled(format!("{delay} ms"), Style::default().fg(color))
        }
        None => Span::styled("-", Style::default().fg(Color::DarkGray)),
    }
}

fn draw_subs(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(area);

    let items: Vec<ListItem> = app
        .subscriptions
        .iter()
        .map(|sub| {
            let provider = app.providers.get(&sub.name);
            let (count, updated, vehicle) = match provider {
                Some(p) => (
                    p.proxies.len().to_string(),
                    if p.updated_at.is_empty() {
                        "-".to_string()
                    } else {
                        p.updated_at.clone()
                    },
                    if p.vehicle_type.is_empty() {
                        "-".to_string()
                    } else {
                        p.vehicle_type.clone()
                    },
                ),
                None => ("-".to_string(), "-".to_string(), "-".to_string()),
            };
            let line = Line::from(vec![
                Span::styled(
                    sub.name.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("组 {}", sub.group_name()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("节点 {count}  更新 {updated}  类型 {vehicle}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let url = Line::from(Span::styled(
                format!("   {}", sub.url),
                Style::default().fg(Color::DarkGray),
            ));
            ListItem::new(vec![line, url])
        })
        .collect();
    let mut state = ListState::default();
    if !app.subscriptions.is_empty() {
        state.select(Some(app.sub_index));
    }
    let list = List::new(items)
        .block(block("订阅"))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[0], &mut state);

    let hint = Paragraph::new(Line::from(
        "a 新增订阅 · u 更新订阅 · h 健康检查 · w 写入配置并重载 · d 删除",
    ))
    .style(Style::default().fg(Color::DarkGray))
    .block(block("操作"));
    frame.render_widget(hint, chunks[1]);
}

fn draw_rules(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).split(area);
    let summary = Paragraph::new(Line::from(vec![
        Span::styled("当前模式：", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.mode_label(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("    按 m 在 规则 → 全局 → 直连 之间切换"),
    ]))
    .block(block("模式"));
    frame.render_widget(summary, chunks[0]);

    let items: Vec<ListItem> = app
        .rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let line = Line::from(vec![
                Span::styled(
                    format!("{:>4}  ", index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<14}", rule.kind),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:<40}", truncate(&rule.payload, 40)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(" → ", Style::default().fg(Color::DarkGray)),
                Span::styled(rule.proxy.clone(), Style::default().fg(Color::Cyan)),
            ]);
            ListItem::new(line)
        })
        .collect();
    let mut state = ListState::default();
    if !app.rules.is_empty() {
        state.select(Some(app.rule_index));
    }
    let rules_title = format!("规则（{}）", app.rules.len());
    let list = List::new(items)
        .block(block(&rules_title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

fn truncate(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        text.to_string()
    } else {
        let mut out: String = chars[..width.saturating_sub(1)].iter().collect();
        out.push('…');
        out
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn draw_connections(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).split(area);

    let up: u64 = app.connections.iter().map(|c| c.upload).sum();
    let down: u64 = app.connections.iter().map(|c| c.download).sum();
    let summary = Paragraph::new(Line::from(vec![
        Span::styled("活动连接：", Style::default().fg(Color::DarkGray)),
        Span::styled(
            app.connections.len().to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        Span::styled(
            format!("本会话上传 {}  下载 {}", human_total(up), human_total(down)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("    "),
        Span::styled("按下载量排序", Style::default().fg(Color::DarkGray)),
    ]))
    .block(block("概览"));
    frame.render_widget(summary, chunks[0]);

    let items: Vec<ListItem> = app
        .connections
        .iter()
        .enumerate()
        .map(|(index, conn)| {
            let kind = match (
                conn.metadata.kind.is_empty(),
                conn.metadata.network.is_empty(),
            ) {
                (false, false) => format!("{}/{}", conn.metadata.kind, conn.metadata.network),
                (false, true) => conn.metadata.kind.clone(),
                (true, false) => conn.metadata.network.clone(),
                (true, true) => "-".to_string(),
            };
            let chain = if conn.chains.is_empty() {
                "-".to_string()
            } else {
                conn.chains.join(" ← ")
            };
            let rule = if conn.rule_payload.is_empty() {
                conn.rule.clone()
            } else {
                format!("{}:{}", conn.rule, conn.rule_payload)
            };
            let process = if conn.metadata.process_path.is_empty() {
                "-".to_string()
            } else {
                basename(&conn.metadata.process_path)
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{:>4} ", index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("{:<9}", truncate(&kind, 9)),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:<32}", truncate(&connection_target(conn), 32)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("↑{:<9}", human_total(conn.upload)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("↓{:<9}", human_total(conn.download)),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<22}", truncate(&chain, 22)),
                    Style::default().fg(Color::Magenta),
                ),
                Span::styled(
                    format!("{:<22}", truncate(&rule, 22)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(truncate(&process, 20), Style::default().fg(Color::Blue)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    if !app.connections.is_empty() {
        state.select(Some(app.conn_index.min(app.connections.len() - 1)));
    }
    let title = format!("活动连接（{}）", app.connections.len());
    let list = List::new(items)
        .block(block(&title))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[1], &mut state);
}

fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let total = app.logs.len();
    let end = total.saturating_sub(app.log_scroll);
    let start = end.saturating_sub(inner_height);
    let lines: Vec<Line> = app
        .logs
        .iter()
        .skip(start)
        .take(end - start)
        .map(|log| {
            let color = match log.level.as_str() {
                "error" => Color::Red,
                "warning" | "warn" => Color::Yellow,
                "debug" => Color::DarkGray,
                _ => Color::White,
            };
            Line::from(vec![
                Span::styled(
                    format!("{:<7} ", log.level),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(log.payload.clone(), Style::default().fg(color)),
            ])
        })
        .collect();
    let title = format!(
        "日志（{}）{}",
        total,
        if app.log_follow {
            " · 跟随中"
        } else {
            " · 已暂停"
        }
    );
    let body = Paragraph::new(lines)
        .block(block(&title))
        .wrap(Wrap { trim: false });
    frame.render_widget(body, area);
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(area);
    let items: Vec<ListItem> = SETTING_FIELDS
        .iter()
        .enumerate()
        .map(|(index, (label, _))| {
            let mut value = setting_value(&app.settings, index);
            if SETTING_FIELDS[index].1 && !value.is_empty() {
                value = "••••••".to_string();
            }
            let line = Line::from(vec![
                Span::styled(format!("{label:<16}"), Style::default().fg(Color::DarkGray)),
                Span::styled(value, Style::default().fg(Color::White)),
            ]);
            ListItem::new(line)
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.setting_index));
    let list = List::new(items)
        .block(block("设置"))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, chunks[0], &mut state);

    let hint = Paragraph::new(Line::from(
        "↑/↓ 选择 · Enter 修改 · w 保存设置（修改后会自动保存）",
    ))
    .style(Style::default().fg(Color::DarkGray))
    .block(block("说明"));
    frame.render_widget(hint, chunks[1]);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    if let Some((message, error)) = &app.message {
        let color = if *error { Color::Red } else { Color::Green };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {message}"),
                Style::default().fg(color),
            ))),
            area,
        );
        return;
    }
    let hints = match app.tab {
        0 => "s 启动 · x 停止 · R 重启 · m 模式 · p 代理开关 · r 刷新 · q 退出",
        1 => "↑/↓ 移动 · Enter 进入/选择 · Esc 返回 · t 测速 · T 整组测速 · q 退出",
        2 => "a 新增 · u 更新 · h 健康检查 · w 应用 · d 删除 · q 退出",
        3 => "↑/↓ 滚动 · m 切换模式 · q 退出",
        4 => "↑/↓ 移动 · Enter/d 关闭选中 · D 关闭全部 · g/G 首尾 · r 刷新 · q 退出",
        5 => "↑/↓ 滚动 · f 跟随 · c 清空 · q 退出",
        6 => "↑/↓ 选择 · Enter 修改 · q 退出",
        _ => "q 退出",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {hints}"),
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn draw_overlay(frame: &mut Frame, overlay: &Overlay, area: Rect) {
    match overlay {
        Overlay::Help => {
            let popup = centered_rect(74, 20, area);
            frame.render_widget(Clear, popup);
            let lines = vec![
                Line::from(Span::styled(
                    "clash-tui 帮助",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from("Tab / 1-7     切换标签页"),
                Line::from("↑/↓ j/k       移动选择"),
                Line::from("Enter         进入代理组 / 选择节点 / 编辑设置"),
                Line::from("d / D         连接页：关闭选中连接 / 关闭全部连接"),
                Line::from("Esc           返回上一级 / 取消"),
                Line::from("s / x / R     启动 / 停止 / 重启内核"),
                Line::from("m             切换模式（规则 → 全局 → 直连）"),
                Line::from("p             开关代理（直连 ↔ 规则）"),
                Line::from("t / T         单节点测速 / 整组测速"),
                Line::from("a / u / h / w 订阅：新增 / 更新 / 健康检查 / 应用"),
                Line::from("r             立即刷新"),
                Line::from("q             退出"),
                Line::from(""),
                Line::from(Span::styled(
                    "内核通过 systemd 用户服务运行，本程序退出后代理继续工作。",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "按 Esc 或 q 关闭本帮助",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block("帮助"))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Overlay::Confirm(confirm) => {
            let popup = centered_rect(60, 7, area);
            frame.render_widget(Clear, popup);
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {}", confirm.message),
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  y / Enter 确认      n / Esc 取消",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block("确认"))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Overlay::Input(input) => {
            let height = (input.fields.len() as u16) + 6;
            let popup = centered_rect(70, height, area);
            frame.render_widget(Clear, popup);
            let mut lines: Vec<Line> = vec![Line::from("")];
            for (index, field) in input.fields.iter().enumerate() {
                let active = index == input.active;
                let value = if field.secret && !field.value.is_empty() {
                    "•".repeat(field.value.chars().count())
                } else {
                    field.value.clone()
                };
                let marker = if active { "▶ " } else { "  " };
                let mut spans = vec![
                    Span::styled(marker, Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{}：", field.label),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        value,
                        if active {
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        },
                    ),
                ];
                if active {
                    spans.push(Span::styled("█", Style::default().fg(Color::Cyan)));
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {}", input.hint),
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block(&input.title))
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
    }
}
