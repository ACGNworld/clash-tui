# clash-tui

`clash-tui` 是 Mihomo/Clash 的终端控制台，用于查看内核状态、切换代理节点、测速、管理订阅和活动连接。

本项目采用 [GNU Lesser General Public License v3.0](LICENSE) 授权。

## 一键安装（Ubuntu / Debian）

克隆仓库后直接运行安装脚本：

```bash
git clone https://github.com/ACGNworld/clash-tui.git
cd clash-tui
./install.sh
```

脚本会依次完成：

1. 检查 `curl`、`python3`、`python3-yaml`、用户服务会话和已有内核冲突，解析配置。安装方式会交互询问：
   - **系统安装**：使用 `sudo dpkg -i`，安装到 `/usr/bin/mihomo`
   - **用户级解包**：无需 `sudo`，二进制放到 `~/.local/bin/mihomo`
2. 下载对应 CPU 架构的 mihomo 内核（默认最新 release），编译 `clash-tui`；编译成功后才安装二进制
3. 生成或补齐 `~/.config/mihomo/config.yaml`；需要修改已有配置时先备份为 `config.yaml.bak`
4. 写入 `~/.config/clash-tui/settings.json`，保留其它设置，让 Controller 地址、secret、服务名、内核路径全部对齐
5. 创建并启用 systemd 用户服务 `mihomo-tui.service`，最多等待 15 秒确认 Controller 连接正常；失败时退出并打印状态和日志

缺少配置处理依赖时先执行 `sudo apt-get install curl python3 python3-yaml`。使用 `XDG_CONFIG_HOME` 时，配置、设置和用户服务文件均写入该目录。

已有系统级 `mihomo.service` 正在运行时，脚本会在安装前退出，提示先停止原服务。没有 systemd 用户会话时，可用 `--skip-service` 只安装；使用 `--skip-build` 时仍会检查 Controller，无需已安装 TUI。默认配置只有 `MATCH,DIRECT`，需要在 TUI 中添加订阅后才能使用代理节点。

安装完成后，新开一个终端即可直接输入 `clash-tui`。常用选项：

```bash
./install.sh --help                       # 查看全部选项
./install.sh --mihomo-version v1.19.32    # 指定内核版本
./install.sh --mirror https://ghfast.top  # GitHub 下载缓慢时使用镜像
./install.sh --deb-mode user              # 跳过询问，改为用户级解包安装
./install.sh --deb-mode user --skip-service # 只安装，不启动服务
```

`--mirror` 只代理内核下载。无法访问 GitHub 查询最新版本时，请同时指定 `--mihomo-version` 和 `--mirror`；版本查询有超时限制，不会无限等待。

> 目前仅支持 Ubuntu/Debian，并且只创建**用户级** systemd 服务；系统级服务和其它发行版会在后续补充。

## 环境要求

- Linux 或其他支持 Rust、`crossterm` 的终端环境
- Rust 和 Cargo
- 已安装 Mihomo/Clash，并开启 External Controller
- 当前用户可以读取和修改 Mihomo 配置文件

示例配置：

```yaml
external-controller: 127.0.0.1:9090
secret: ""
```

## 编译和启动

在项目目录执行：

```bash
cargo build --release
./target/release/clash-tui
```

也可以直接运行：

```bash
cargo run --release
```

启动参数：

```text
--url <地址>       Controller 地址，默认 http://127.0.0.1:9090
--secret <密钥>    Controller secret
--service <单元>   systemd 用户单元名，默认 mihomo-tui.service
--config <路径>    Mihomo 配置文件路径
--check            只检查 Controller 连接并打印版本
-h, --help         显示帮助
```

例如：

```bash
./target/release/clash-tui \
  --url http://127.0.0.1:9090 \
  --secret your-secret \
  --config ~/.config/mihomo/config.yaml
```

也可以使用环境变量：

```bash
CLASH_CONTROLLER=http://127.0.0.1:9090 \
CLASH_SECRET=your-secret \
./target/release/clash-tui
```

先检查连接：

```bash
./target/release/clash-tui --check
```

## 常用操作

程序启动后使用 `Tab` 或数字键 `1` 到 `7` 切换页面。使用 `↑/↓` 或 `j/k` 移动，按 `?` 查看帮助，按 `q` 退出。

- **状态**：查看内核、Controller、代理模式、当前节点、上下行速率和活动连接数。`s` 启动内核，`x` 停止内核，`R` 重启，`m` 切换规则/全局/直连模式，`p` 开关代理。
- **代理**：进入代理组后按 `Enter` 选择节点；`t` 测试当前节点，`T` 测试整组节点，`Esc` 返回代理组列表。
- **订阅**：`a` 新增订阅，`u` 更新订阅，`h` 执行健康检查，`w` 写入托管配置并尝试热重载，`d` 删除订阅。
- **规则**：查看当前规则和命中模式；按 `m` 切换代理模式。
- **连接**：查看活动连接；按 `Enter` 或 `d` 关闭选中连接，按 `D` 关闭全部连接。
- **日志**：查看日志；`f` 切换跟随，`↑/↓` 滚动，`c` 清空当前显示。
- **设置**：选择字段后按 `Enter` 修改。设置会保存到本地；测速地址、超时、Controller 地址、secret、配置文件路径和内核二进制路径都可以修改。

## 文件位置

默认情况下：

- Mihomo 配置：`~/.config/mihomo/config.yaml`
- clash-tui 设置：`~/.config/clash-tui/settings.json`
- 订阅列表：`~/.config/clash-tui/subscriptions.json`
- 内核日志：`~/.config/clash-tui/logs/mihomo.log`

订阅应用会在配置文件中维护带标记的托管区块，并生成 `.bak` 备份。不要手动修改托管区块中的内容；再次应用订阅时会被覆盖。

## 内核管理说明

程序优先通过 `systemctl --user` 管理 `mihomo-tui.service`。没有可用的 systemd 用户会话时，会尝试使用 `setsid` 将 Mihomo 脱离终端启动，因此退出 TUI 不会自动停止代理。

如果 Mihomo 已经在运行，直接把 Controller 地址和 secret 填正确即可接管，不需要再次启动内核。

切换到规则或全局模式时，程序会自动维护 `~/.bashrc` 中带标记的代理声明区块；切换到直连模式时会移除该区块。新开的 Bash 终端会自动读取，当前终端可执行 `source ~/.bashrc` 立即生效。其他 Shell 请写入对应的启动文件。

代理端口以状态页显示的 mixed-port 为准。自动维护的声明等价于：

```bash
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890
```

使用 `curl -I https://www.google.com` 可以验证连接。

不要使用 `ping` 验证 HTTP 代理。`ping` 使用 ICMP，不会读取上述环境变量，也不会经过 Mihomo 的 HTTP mixed-port；目标站点不响应 ICMP 时出现丢包是正常现象。请使用 `curl`、`wget`、浏览器或其他支持 HTTP/SOCKS 代理的程序测试。

## 当前限制

- TUN 模式和系统代理接管尚未实现。
- `/logs` 和 `/traffic` 在 Mihomo 中使用 WebSocket；当前版本保留了流式读取代码，实时数据能力取决于内核是否接受该连接方式。
- 订阅托管区块适用于 proxy-provider 配置；复杂的完整 YAML 配置仍建议先在 Mihomo 中验证。

## 开发检查

```bash
python3 -m unittest discover -s tests -v
cargo fmt -- --check
cargo test --offline
cargo clippy --offline --all-targets --all-features -- -D warnings
```
