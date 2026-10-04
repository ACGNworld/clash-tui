#!/usr/bin/env bash
#
# clash-tui 一键安装脚本（Ubuntu / Debian）
#
# 依次完成：
#   1. 检测 CPU 架构，下载对应版本的 mihomo .deb（默认取 GitHub 最新 release）
#   2. 安装 mihomo 内核（交互选择：sudo 系统安装 / 用户级解包）
#   3. 创建并启用 systemd“用户级”服务 mihomo-tui.service
#   4. 编译 clash-tui 并安装到 ~/.local/bin，确保 PATH 可直接调用
#   5. 写入/更新 clash-tui 设置，使内核、Controller、secret 全部对齐
#
# 用法：
#   ./install.sh [选项]
#
# 选项：
#   --deb-mode <system|user>   跳过交互，直接指定内核安装方式
#   --mihomo-version <tag>     指定 mihomo 版本（例如 v1.19.32），默认最新
#   --mirror <前缀>            GitHub 下载镜像前缀，例如 https://ghfast.top
#   --skip-build               跳过编译 clash-tui（只装内核和服务）
#   --skip-service             只安装二进制，不创建/启动 systemd 用户服务
#   --yes                      非交互模式，全部使用默认选项
#   -h, --help                 显示本帮助
#
# 环境变量：
#   MIHOMO_VERSION / MIHOMO_MIRROR 等价于对应选项
#
set -euo pipefail

# ---------------------------------------------------------------------------
# 基本工具与日志
# ---------------------------------------------------------------------------

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CURRENT_USER="${USER:-$(id -un)}"
HOME_DIR="${HOME:-$(getent passwd "$CURRENT_USER" | cut -d: -f6)}"

SERVICE_NAME="mihomo-tui.service"
CONFIG_DIR="$HOME_DIR/.config/mihomo"
CONFIG_FILE="$CONFIG_DIR/config.yaml"
SETTINGS_FILE="$HOME_DIR/.config/clash-tui/settings.json"
BIN_DIR="$HOME_DIR/.local/bin"

CONTROLLER="127.0.0.1:9090"
SECRET=""
URL=""
BINARY=""

DEB_MODE=""
MIHOMO_VERSION="${MIHOMO_VERSION:-}"
MIRROR="${MIHOMO_MIRROR:-}"
ASSUME_YES=0
SKIP_BUILD=0
SKIP_SERVICE=0

ARCH=""
ARCH_DESC=""
ASSET=""
DEB_FILE=""

if [ -t 1 ]; then
  C_RED=$'\033[1;31m'; C_GREEN=$'\033[1;32m'; C_YELLOW=$'\033[1;33m'
  C_BLUE=$'\033[1;34m'; C_RESET=$'\033[0m'
else
  C_RED=""; C_GREEN=""; C_YELLOW=""; C_BLUE=""; C_RESET=""
fi

info() { printf '%s[信息]%s %s\n' "$C_BLUE" "$C_RESET" "$*"; }
ok()   { printf '%s[完成]%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s[警告]%s %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
err()  { printf '%s[错误]%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; }
die()  { err "$*"; exit 1; }

usage() {
  cat <<'EOF'
clash-tui 一键安装脚本（Ubuntu / Debian）

用法：
  ./install.sh [选项]

选项：
  --deb-mode <system|user>   跳过交互，直接指定内核安装方式
  --mihomo-version <tag>     指定 mihomo 版本（例如 v1.19.32），默认最新
  --mirror <前缀>            GitHub 下载镜像前缀，例如 https://ghfast.top
  --skip-build               跳过编译 clash-tui（只装内核和服务）
  --skip-service             只安装二进制，不创建/启动 systemd 用户服务
  --yes                      非交互模式，全部使用默认选项
  -h, --help                 显示本帮助

环境变量：
  MIHOMO_VERSION / MIHOMO_MIRROR 等价于对应选项
EOF
}

# ---------------------------------------------------------------------------
# 参数解析
# ---------------------------------------------------------------------------

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --deb-mode)
        shift
        DEB_MODE="${1:-}"
        case "$DEB_MODE" in
          system|user) ;;
          *) die "--deb-mode 只能是 system 或 user" ;;
        esac
        ;;
      --deb-mode=*)
        DEB_MODE="${1#*=}"
        case "$DEB_MODE" in
          system|user) ;;
          *) die "--deb-mode 只能是 system 或 user" ;;
        esac
        ;;
      --mihomo-version)
        shift; MIHOMO_VERSION="${1:-}"
        ;;
      --mihomo-version=*)
        MIHOMO_VERSION="${1#*=}"
        ;;
      --mirror)
        shift; MIRROR="${1:-}"
        ;;
      --mirror=*)
        MIRROR="${1#*=}"
        ;;
      --skip-build)
        SKIP_BUILD=1
        ;;
      --skip-service)
        SKIP_SERVICE=1
        ;;
      --yes|-y)
        ASSUME_YES=1
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "未知参数：$1（使用 --help 查看用法）"
        ;;
    esac
    shift
  done
}

# 交互读取；--yes 时直接返回默认值。提示写到 stderr，避免污染命令替换结果。
ask() { # <提示> <默认值>
  local prompt="$1" default="$2" answer=""
  if [ "$ASSUME_YES" = 1 ]; then
    printf '%s' "$default"
    return 0
  fi
  read -r -p "$prompt" answer || answer=""
  printf '%s' "${answer:-$default}"
}

# ---------------------------------------------------------------------------
# 环境检测
# ---------------------------------------------------------------------------

detect_os() {
  [ -f /etc/os-release ] || die "无法识别系统：缺少 /etc/os-release"
  # shellcheck disable=SC1091
  . /etc/os-release
  command -v dpkg-deb >/dev/null 2>&1 || die "未找到 dpkg-deb，本脚本需要 Ubuntu/Debian 环境"
  case "${ID:-} ${ID_LIKE:-}" in
    *ubuntu*|*debian*) ;;
    *) warn "当前系统为 ${PRETTY_NAME:-未知}，脚本仅在 Ubuntu/Debian 上验证过，继续执行可能失败。" ;;
  esac
  if [ "$(id -u)" = "0" ]; then
    warn "检测到以 root 身份运行。用户级 systemd 服务会安装到 root 的家目录下，通常不是你想要的结果。"
  fi
}

detect_arch() {
  local machine variant=""
  machine="$(uname -m)"
  case "$machine" in
    x86_64|amd64)  ARCH="amd64"; variant="$(detect_amd64_variant)" ;;
    aarch64|arm64) ARCH="arm64" ;;
    armv7l|armv7)  ARCH="armv7" ;;
    armv6l|armv6)  ARCH="armv6" ;;
    i386|i486|i586|i686) ARCH="386" ;;
    *) die "暂不支持的 CPU 架构：$machine" ;;
  esac
  ARCH_DESC="${ARCH}${variant}"
  ASSET="mihomo-linux-${ARCH}${variant}-${MIHOMO_VERSION}.deb"
}

# amd64 上按 CPU 指令集选择 v3 / v2 / 基线版本
detect_amd64_variant() {
  local flags
  flags="$(awk -F: '/^flags/{print $2; exit}' /proc/cpuinfo 2>/dev/null || true)"
  [ -n "$flags" ] || { printf '%s' ""; return 0; }
  has() { case " $flags " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
  if has avx && has avx2 && has bmi1 && has bmi2 && has fma && has movbe && has f16c; then
    printf '%s' "-v3"
  elif has sse4_2 && has popcnt && has cx16; then
    printf '%s' "-v2"
  else
    printf '%s' ""
  fi
}

resolve_version() {
  if [ -z "$MIHOMO_VERSION" ]; then
    local loc=""
    loc="$(curl -fsSI -o /dev/null -w '%{redirect_url}' \
      "https://github.com/MetaCubeX/mihomo/releases/latest" 2>/dev/null || true)"
    if [ -z "$loc" ]; then
      loc="$(curl -fsSL "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest" 2>/dev/null \
        | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/' || true)"
    fi
    MIHOMO_VERSION="${loc##*/}"
  fi
  case "$MIHOMO_VERSION" in
    v[0-9]*) ;;
    *) die "无法获取 mihomo 版本，请用 --mihomo-version vX.Y.Z 手动指定" ;;
  esac
}

# ---------------------------------------------------------------------------
# 下载与安装 mihomo
# ---------------------------------------------------------------------------

download_deb() {
  local url out
  out="$TMP_DIR/${ASSET}"
  url="https://github.com/MetaCubeX/mihomo/releases/download/${MIHOMO_VERSION}/${ASSET}"
  if [ -n "$MIRROR" ]; then
    url="${MIRROR%/}/$url"
  fi
  info "下载 $url"
  curl -fL --retry 3 --retry-delay 2 --connect-timeout 15 \
    --progress-bar -o "$out" "$url" \
    || die "下载失败。可尝试加镜像：--mirror https://ghfast.top"
  dpkg-deb -I "$out" >/dev/null 2>&1 || die "下载的文件不是有效的 deb 包：$out"
  DEB_FILE="$out"
  ok "已下载 ${ASSET}"
}

choose_deb_mode() {
  if [ -n "$DEB_MODE" ]; then
    printf '%s' "$DEB_MODE"
    return 0
  fi
  if [ "$ASSUME_YES" = 1 ]; then
    printf '%s' "system"
    return 0
  fi
  {
    printf '\n请选择 mihomo 内核的安装方式：\n'
    printf '  1) 系统安装   使用 sudo dpkg -i，安装到 /usr/bin/mihomo（所有用户可用）\n'
    printf '  2) 用户级解包 无需 sudo，二进制放到 ~/.local/bin/mihomo（仅当前用户）\n'
  } >&2
  local answer
  read -r -p "请输入 [1/2]（默认 1）: " answer || answer=""
  case "$answer" in
    2) printf '%s' "user" ;;
    *) printf '%s' "system" ;;
  esac
}

install_mihomo() {
  local mode
  mode="$(choose_deb_mode)"
  case "$mode" in
    system) install_mihomo_system ;;
    user)   install_mihomo_user ;;
    *) die "无效的安装方式：$mode" ;;
  esac
}

install_mihomo_system() {
  command -v sudo >/dev/null 2>&1 || die "系统安装需要 sudo，但未找到 sudo；可重新运行并选择用户级解包"
  info "使用 sudo 安装 deb 包（安装到 /usr/bin/mihomo）..."
  sudo -v || die "sudo 认证失败"
  if ! sudo DEBIAN_FRONTEND=noninteractive dpkg --force-confdef --force-confold -i "$DEB_FILE"; then
    warn "dpkg 报告依赖问题，尝试用 apt 修复..."
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -f -y \
      || die "依赖修复失败，请手动执行：sudo apt-get -f install"
  fi
  [ -x /usr/bin/mihomo ] || die "安装完成但未找到 /usr/bin/mihomo"
  BINARY="/usr/bin/mihomo"
  ok "mihomo 已安装：$BINARY"
}

install_mihomo_user() {
  local root="$TMP_DIR/deb-root"
  info "解包 deb 并安装到 $BIN_DIR（无需 sudo）..."
  rm -rf "$root"
  dpkg-deb -x "$DEB_FILE" "$root"
  [ -x "$root/usr/bin/mihomo" ] || die "deb 中未找到 mihomo 二进制"
  mkdir -p "$BIN_DIR"
  install -m 0755 "$root/usr/bin/mihomo" "$BIN_DIR/mihomo"
  BINARY="$BIN_DIR/mihomo"
  ok "mihomo 已安装：$BINARY"
}

# ---------------------------------------------------------------------------
# 配置 mihomo（external-controller / secret）
# ---------------------------------------------------------------------------

gen_secret() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 16
  else
    head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n'
  fi
}

# 读取 YAML 顶层键的值（仅匹配行首、不缩进的 key:）
top_value() {
  local key="$1" line
  line="$(grep -m1 -E "^${key}:" "$CONFIG_FILE" 2>/dev/null || true)"
  [ -n "$line" ] || return 1
  line="${line#*:}"
  line="${line#"${line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  line="${line%\"}"; line="${line#\"}"
  line="${line%\'}"; line="${line#\'}"
  printf '%s' "$line"
}

setup_config() {
  mkdir -p "$CONFIG_DIR"
  if [ ! -f "$CONFIG_FILE" ]; then
    SECRET="$(gen_secret)"
    cat > "$CONFIG_FILE" <<EOF
mixed-port: 7890
allow-lan: false
mode: rule
log-level: info
external-controller: ${CONTROLLER}
secret: ${SECRET}

dns:
  enable: true
  ipv6: true
  enhanced-mode: fake-ip
  nameserver:
    - system

rules:
  - MATCH,DIRECT
EOF
    ok "已生成默认配置 $CONFIG_FILE"
  fi

  # 确保存在 external-controller
  local ec=""
  ec="$(top_value external-controller || true)"
  if [ -n "$ec" ]; then
    CONTROLLER="$ec"
  else
    printf '\nexternal-controller: %s\n' "$CONTROLLER" >> "$CONFIG_FILE"
    info "已在配置中补充 external-controller: $CONTROLLER"
  fi

  # 确保存在 secret（保留已有值，即使是空值）
  if grep -qE '^secret:' "$CONFIG_FILE"; then
    SECRET="$(top_value secret || true)"
  else
    [ -n "$SECRET" ] || SECRET="$(gen_secret)"
    printf 'secret: %s\n' "$SECRET" >> "$CONFIG_FILE"
    info "已在配置中补充 secret"
  fi

  # 归一化 Controller 地址
  case "$CONTROLLER" in
    http://*|https://*) URL="$CONTROLLER" ;;
    *) URL="http://$CONTROLLER" ;;
  esac
  URL="${URL/0.0.0.0/127.0.0.1}"
}

# ---------------------------------------------------------------------------
# systemd 用户级服务
# ---------------------------------------------------------------------------

write_user_service() {
  local unit_dir="$HOME_DIR/.config/systemd/user"
  local unit="$unit_dir/$SERVICE_NAME"
  mkdir -p "$unit_dir"
  cat > "$unit" <<EOF
[Unit]
Description=Mihomo core for clash-tui
Documentation=https://github.com/ACGNworld/clash-tui
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart="$BINARY" -d "$CONFIG_DIR" -f "$CONFIG_FILE"
Restart=on-failure
RestartSec=5
LimitNOFILE=infinity

[Install]
WantedBy=default.target
EOF
  chmod 644 "$unit"
  ok "已写入 $unit"
}

enable_user_service() {
  if ! systemctl --user show-environment >/dev/null 2>&1; then
    warn "当前会话没有可用的 systemd 用户总线（可能通过 su / 非登录 SSH 运行）。"
    warn "请在有登录会话的终端中执行："
    warn "  systemctl --user daemon-reload && systemctl --user enable --now $SERVICE_NAME"
    return 1
  fi
  systemctl --user daemon-reload
  systemctl --user enable "$SERVICE_NAME" >/dev/null 2>&1 || true
  systemctl --user restart "$SERVICE_NAME" \
    || die "无法启动 $SERVICE_NAME，请查看：systemctl --user status $SERVICE_NAME"
  sleep 1
  if systemctl --user is-active --quiet "$SERVICE_NAME"; then
    ok "systemd 用户服务 $SERVICE_NAME 已启动"
  else
    warn "服务未处于 active 状态，请检查：systemctl --user status $SERVICE_NAME"
  fi
}

enable_linger() {
  command -v loginctl >/dev/null 2>&1 || return 0
  if loginctl enable-linger "$CURRENT_USER" >/dev/null 2>&1; then
    info "已开启 linger：注销或重启后用户服务仍会自动运行"
  else
    warn "未能自动开启 linger（可选）。如需注销后继续运行：sudo loginctl enable-linger $CURRENT_USER"
  fi
}

warn_conflicting_kernel() {
  if systemctl is-active --quiet mihomo 2>/dev/null; then
    warn "检测到系统级 mihomo.service 正在运行，可能与用户级服务抢占同一端口。"
    warn "建议先停止：sudo systemctl disable --now mihomo"
  fi
}

# ---------------------------------------------------------------------------
# 编译并安装 clash-tui
# ---------------------------------------------------------------------------

ensure_build_tools() {
  if command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1; then
    return 0
  fi
  warn "未检测到 C 编译器（cc/gcc），Rust 链接需要它。"
  if command -v sudo >/dev/null 2>&1; then
    local answer
    answer="$(ask "是否使用 sudo 安装 build-essential？[y/N]: " "n")"
    case "$answer" in
      y|Y|yes|YES)
        sudo apt-get update -qq && sudo apt-get install -y build-essential ;;
      *) warn "已跳过，编译可能失败" ;;
    esac
  else
    warn "请手动安装 build-essential 后重试"
  fi
}

cargo_works() {
  command -v cargo >/dev/null 2>&1 && cargo --version >/dev/null 2>&1
}

ensure_rust() {
  if [ -f "$HOME_DIR/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME_DIR/.cargo/env"
  fi
  if cargo_works; then
    return 0
  fi
  # 已装 rustup 但缺少默认工具链时，cargo 会报错而不是缺失
  if command -v rustup >/dev/null 2>&1; then
    info "检测到 rustup 但缺少默认工具链，正在安装 stable..."
    rustup default stable >/dev/null 2>&1 || true
    if cargo_works; then
      ok "Rust 工具链已就绪：$(cargo --version)"
      return 0
    fi
  fi
  info "未检测到可用的 Rust/Cargo，使用 rustup 安装到 ~/.cargo（无需 sudo）..."
  curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o "$TMP_DIR/rustup-init.sh" \
    || die "下载 rustup 失败"
  sh "$TMP_DIR/rustup-init.sh" -y --profile minimal --default-toolchain stable \
    || die "rustup 安装失败"
  # shellcheck disable=SC1091
  . "$HOME_DIR/.cargo/env"
  cargo_works || die "Rust 安装后仍无法运行 cargo"
  ok "Rust 工具链已就绪：$(cargo --version)"
}

build_clash_tui() {
  ensure_build_tools
  ensure_rust
  info "编译 clash-tui（release，首次编译可能需要几分钟）..."
  ( cd "$REPO_ROOT" && cargo build --release --locked )
  [ -x "$REPO_ROOT/target/release/clash-tui" ] || die "未找到编译产物 target/release/clash-tui"
  mkdir -p "$BIN_DIR"
  install -m 0755 "$REPO_ROOT/target/release/clash-tui" "$BIN_DIR/clash-tui"
  ok "已安装 clash-tui：$BIN_DIR/clash-tui"
}

ensure_path() {
  if printf '%s' "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    return 0
  fi
  local marker="clash-tui: ensure ~/.local/bin in PATH"
  # 需要在 shell 启动文件中保留字面量 $HOME / $PATH，因此这里用单引号
  # shellcheck disable=SC2016
  local line='export PATH="$HOME/.local/bin:$PATH"'
  local rc
  for rc in "$HOME_DIR/.bashrc" "$HOME_DIR/.profile" "$HOME_DIR/.zshrc"; do
    [ -e "$rc" ] || continue
    grep -q "$marker" "$rc" 2>/dev/null && continue
    {
      printf '\n# >>> %s >>>\n' "$marker"
      printf '%s\n' "$line"
      printf '# <<< %s <<<\n' "$marker"
    } >> "$rc"
    info "已将 ~/.local/bin 写入 PATH：$rc"
  done
  if [ ! -e "$HOME_DIR/.bashrc" ] && [ ! -e "$HOME_DIR/.profile" ]; then
    {
      printf '\n# >>> %s >>>\n' "$marker"
      printf '%s\n' "$line"
      printf '# <<< %s <<<\n' "$marker"
    } >> "$HOME_DIR/.profile"
    info "已将 ~/.local/bin 写入 PATH：$HOME_DIR/.profile"
  fi
}

# ---------------------------------------------------------------------------
# 写入 clash-tui 设置
# ---------------------------------------------------------------------------

update_settings() {
  mkdir -p "$(dirname "$SETTINGS_FILE")"
  if [ ! -f "$SETTINGS_FILE" ]; then
    cat > "$SETTINGS_FILE" <<EOF
{
  "url": "$URL",
  "secret": "$SECRET",
  "service": "$SERVICE_NAME",
  "config_path": "$CONFIG_FILE",
  "binary": "$BINARY"
}
EOF
    chmod 600 "$SETTINGS_FILE"
    ok "已写入设置 $SETTINGS_FILE"
    return 0
  fi

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$SETTINGS_FILE" "$URL" "$SECRET" "$SERVICE_NAME" "$CONFIG_FILE" "$BINARY" <<'PY'
import json
import sys

path, url, secret, service, config, binary = sys.argv[1:7]
with open(path, encoding="utf-8") as handle:
    data = json.load(handle)
data.update({
    "url": url,
    "secret": secret,
    "service": service,
    "config_path": config,
    "binary": binary,
})
with open(path, "w", encoding="utf-8") as handle:
    json.dump(data, handle, indent=2, ensure_ascii=False)
    handle.write("\n")
PY
    chmod 600 "$SETTINGS_FILE"
    ok "已更新设置 $SETTINGS_FILE"
  else
    warn "未找到 python3，无法合并已有设置。请手动确认 $SETTINGS_FILE 中的以下字段："
    warn "  url=$URL secret=$SECRET service=$SERVICE_NAME config_path=$CONFIG_FILE binary=$BINARY"
  fi
}

final_check() {
  [ -x "$BIN_DIR/clash-tui" ] || return 0
  info "运行 clash-tui --check 验证 Controller 连接..."
  if "$BIN_DIR/clash-tui" --check; then
    ok "Controller 连接正常，安装完成。"
  else
    warn "clash-tui --check 失败。内核可能还在启动或 secret 不一致，请稍后重试："
    warn "  systemctl --user status $SERVICE_NAME"
    warn "  $BIN_DIR/clash-tui --check"
  fi
}

# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------

print_summary() {
  cat <<EOF

$(ok '安装完成')
  内核二进制 : ${BINARY}
  配置文件   : ${CONFIG_FILE}
  用户服务   : ${SERVICE_NAME}
  clash-tui  : ${BIN_DIR}/clash-tui

常用命令：
  clash-tui                          # 打开终端控制台（新终端里可直接用）
  systemctl --user status  ${SERVICE_NAME}
  systemctl --user restart ${SERVICE_NAME}
  journalctl --user -u ${SERVICE_NAME} -f

如果当前终端还不能直接输入 clash-tui，执行：
  source ~/.bashrc      # 或重新打开一个终端
EOF
}

main() {
  parse_args "$@"

  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/clash-tui-install.XXXXXX")"
  cleanup() { rm -rf "$TMP_DIR"; }
  trap cleanup EXIT

  detect_os
  resolve_version
  detect_arch
  info "将安装 mihomo ${MIHOMO_VERSION}（${ARCH_DESC}）"
  download_deb
  install_mihomo
  setup_config
  warn_conflicting_kernel

  if [ "$SKIP_SERVICE" = 0 ]; then
    write_user_service
    enable_user_service || true
    enable_linger
  else
    info "已跳过 systemd 服务创建（--skip-service）"
  fi

  if [ "$SKIP_BUILD" = 0 ]; then
    build_clash_tui
    ensure_path
  else
    info "已跳过 clash-tui 编译（--skip-build）"
  fi

  update_settings
  final_check
  print_summary
}

# 直接执行时才运行 main；被 source 时可复用其中的函数（便于测试）
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
