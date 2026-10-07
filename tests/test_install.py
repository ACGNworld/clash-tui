"""Installer regression checks; all installation/service calls are isolated."""

import json
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[1]
PREPARE = runpy.run_path(str(ROOT / "scripts/install_config.py"))["prepare"]

# Keep real configuration, file installation, and orchestration, but never invoke
# package managers, network downloads, or the host's systemd services.
HARNESS = r'''
source "$TEST_ROOT/install.sh"
HOME_DIR="$TEST_HOME"
BIN_DIR="$HOME_DIR/.local/bin"
systemctl() {
  printf '%s\n' "$*" >> "$TEST_LOG"
  case "$*" in
    "--user show-environment") [ "${NO_BUS:-0}" = 0 ] ;;
    "is-active --quiet mihomo") [ "${CONFLICT:-0}" = 1 ] ;;
    "--user daemon-reload") [ "${RELOAD_FAIL:-0}" = 0 ] ;;
    "--user enable "*) [ "${ENABLE_FAIL:-0}" = 0 ] ;;
    "--user restart "*) [ "${RESTART_FAIL:-0}" = 0 ] ;;
    "--user is-active --quiet "*) [ "${SERVICE_INACTIVE:-0}" = 0 ] ;;
    "--user status "*) printf 'mock service status\n' ;;
    *) return 0 ;;
  esac
}
journalctl() { printf 'mock recent logs\n'; }
curl() {
  printf 'curl %s\n' "$*" >> "$TEST_LOG"
  CURL_ATTEMPTS=$((${CURL_ATTEMPTS:-0} + 1))
  if [ "${CURL_FAIL:-0}" = 1 ] || [ "$CURL_ATTEMPTS" -le "${CURL_DELAY:-0}" ]; then
    printf 'mock controller unavailable\n' >&2
    return 22
  fi
  local out=""
  while [ $# -gt 0 ]; do
    if [ "$1" = -o ]; then out="$2"; shift; fi
    shift
  done
  if [ "${INVALID_RESPONSE:-0}" = 1 ]; then
    printf '<html>wrong endpoint</html>' > "$out"
  else
    printf '{"version":"v1.19.32"}' > "$out"
  fi
}
sleep() { SECONDS=$((SECONDS + 5)); }
download_deb() { printf 'download\n' >> "$TEST_LOG"; }
build_clash_tui() {
  printf 'build\n' >> "$TEST_LOG"
  [ "${BUILD_FAIL:-0}" = 0 ] || die 'mock build failure'
  BUILD_BINARY="$TEST_BINARY"
}
install_mihomo() {
  printf 'install kernel\n' >> "$TEST_LOG"
  mkdir -p "$BIN_DIR"
  install -m 755 "$TEST_BINARY" "$BINARY"
}
ensure_path() { printf 'path\n' >> "$TEST_LOG"; }
enable_linger() { printf 'linger\n' >> "$TEST_LOG"; }
main "$@"
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="clash-tui-install-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = self.root / "custom config/mihomo/config.yaml"
        self.settings = self.root / "custom config/clash-tui/settings.json"
        self.output = self.root / "prepared"
        self.config.parent.mkdir(parents=True)
        self.binary = self.root / "dummy binary"
        self.binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        self.binary.chmod(0o755)
        self.log = self.root / "calls.log"

    def prepare(self):
        PREPARE(self.config, self.settings, self.output, "127.0.0.1:9090", "mihomo-tui.service", str(self.binary))
        return json.loads((self.output / "settings.json").read_text(encoding="utf-8"))

    def run_installer(self, *args, **options):
        env = os.environ.copy()
        env.update(
            TEST_ROOT=str(ROOT),
            TEST_HOME=str(self.root / "user home"),
            TEST_LOG=str(self.log),
            TEST_BINARY=str(self.binary),
            XDG_CONFIG_HOME=str(self.config.parent.parent),
            MIHOMO_VERSION="",
            MIHOMO_MIRROR="",
        )
        env.update({key: str(value) for key, value in options.items()})
        return subprocess.run(
            ["bash", "-c", HARNESS, "test", "--deb-mode", "user", "--mihomo-version", "v1.19.32", *args],
            env=env, text=True, capture_output=True, timeout=10,
        )

    def calls(self):
        return self.log.read_text(encoding="utf-8") if self.log.exists() else ""

    def test_default_config(self):
        settings = self.prepare()
        config = yaml.safe_load((self.output / "config.yaml").read_text(encoding="utf-8"))
        self.assertEqual(config["rules"], ["MATCH,DIRECT"])
        self.assertEqual(config["secret"], settings["secret"])
        self.assertEqual(len(settings["secret"]), 32)

    def test_comments_escapes_and_other_settings(self):
        original = 'external-controller: "0.0.0.0:9090" # local\nsecret: \'a"b\\c\' # auth\nrules: [MATCH,DIRECT]\n'
        self.config.write_text(original, encoding="utf-8")
        self.settings.parent.mkdir(parents=True)
        self.settings.write_text('{"workers": 4, "test_url": "https://example.com"}', encoding="utf-8")
        settings = self.prepare()
        self.assertEqual(settings["url"], "http://127.0.0.1:9090")
        self.assertEqual(settings["secret"], 'a"b\\c')
        self.assertEqual(settings["workers"], 4)
        self.assertEqual((self.output / "config.yaml").read_text(encoding="utf-8"), original)
        self.assertEqual(self.config.read_text(encoding="utf-8"), original)

    def test_empty_controller_is_replaced_and_empty_secret_preserved(self):
        self.config.write_text('external-controller: # unset\nsecret: "" # no auth\nrules: [MATCH,DIRECT]\n', encoding="utf-8")
        settings = self.prepare()
        text = (self.output / "config.yaml").read_text(encoding="utf-8")
        self.assertEqual(text.count("external-controller:"), 1)
        self.assertEqual(settings["secret"], "")
        self.assertEqual(yaml.safe_load(text)["external-controller"], "127.0.0.1:9090")

    def test_ipv6_and_empty_listening_host(self):
        for host in ("[::]:9090", ":9090"):
            with self.subTest(host=host):
                self.config.write_text(f'external-controller: "{host}"\nsecret: ""\n', encoding="utf-8")
                self.assertEqual(self.prepare()["url"], "http://127.0.0.1:9090")

    def test_repair_preserves_comments_and_managed_subscription_markers(self):
        original = (
            "external-controller: # unset\n"
            "# >>> clash-tui managed block >>>\n"
            "proxy-providers: {} # subscription metadata\n"
            "# <<< clash-tui managed block <<<\n"
            "rules: [MATCH,DIRECT]\n...\n"
        )
        self.config.write_text(original, encoding="utf-8")
        self.prepare()
        text = (self.output / "config.yaml").read_text(encoding="utf-8")
        self.assertIn("# unset", text)
        self.assertIn(original[original.index("# >>>"):original.index("...")], text)
        self.assertTrue(text.endswith("...\n"))
        self.assertIn("secret", yaml.safe_load(text))

    def test_empty_values_aliases_anchors_and_flow_mappings(self):
        cases = (
            "external-controller:\nsecret: ''\n",
            "external-controller:",
            "empty: &empty ''\nexternal-controller: *empty\nsecret: ''\n",
            "external-controller: &controller\nsecret: ''\n",
            "external-controller: !!null null\nsecret: ''\n",
            "{external-controller: '', rules: [MATCH,DIRECT]}",
            "{rules: [MATCH,DIRECT],}",
            "# empty config\n",
            "---\n",
            "{}",
        )
        for original in cases:
            with self.subTest(original=original):
                self.config.write_text(original, encoding="utf-8")
                settings = self.prepare()
                config = yaml.safe_load((self.output / "config.yaml").read_text(encoding="utf-8"))
                self.assertEqual(config["external-controller"], "127.0.0.1:9090")
                self.assertEqual(settings["url"], "http://127.0.0.1:9090")

    def test_invalid_configuration_fails_before_install(self):
        for text in ("rules: [", "- wrong root", "secret: a\nsecret: b\n"):
            with self.subTest(text=text):
                self.config.write_text(text, encoding="utf-8")
                result = self.run_installer()
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("download", self.calls())
                self.assertNotIn("install kernel", self.calls())
                self.assertEqual(self.config.read_text(encoding="utf-8"), text)

    def test_invalid_settings_fails_before_install(self):
        self.settings.parent.mkdir(parents=True)
        self.settings.write_text("invalid json", encoding="utf-8")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("install kernel", self.calls())

    def test_build_failure_keeps_existing_installation(self):
        original = 'external-controller: 127.0.0.1:9090\nsecret: old\n'
        self.config.write_text(original, encoding="utf-8")
        result = self.run_installer(BUILD_FAIL=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("install kernel", self.calls())
        self.assertNotIn("--user restart", self.calls())
        self.assertEqual(self.config.read_text(encoding="utf-8"), original)

    def test_system_service_conflict_blocks_before_download(self):
        result = self.run_installer(CONFLICT=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("sudo systemctl disable --now mihomo", result.stderr)
        self.assertNotIn("download", self.calls())

    def test_missing_user_bus_fails_early(self):
        result = self.run_installer(NO_BUS=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("download", self.calls())

    def test_skip_service_allows_no_bus_and_existing_system_service(self):
        result = self.run_installer("--skip-service", "--skip-build", NO_BUS=1, CONFLICT=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--skip-service", result.stdout)
        self.assertIn("未安装（--skip-build）", result.stdout)
        self.assertNotIn("curl", self.calls())
        self.assertNotIn("--user restart", self.calls())
        self.assertNotIn("linger", self.calls())
        self.assertTrue(self.settings.exists())

    def test_service_command_failures_are_reported(self):
        for option in ("RELOAD_FAIL", "ENABLE_FAIL", "RESTART_FAIL"):
            with self.subTest(option=option):
                result = self.run_installer(**{option: 1})
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("安装完成", result.stdout)
                self.assertIn("mock service status", result.stderr)
                self.assertIn("mock recent logs", result.stderr)

    def test_controller_failures_are_reported(self):
        for option in ("SERVICE_INACTIVE", "CURL_FAIL", "INVALID_RESPONSE"):
            with self.subTest(option=option):
                result = self.run_installer("--skip-build", **{option: 1})
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn("安装完成", result.stdout)
                self.assertIn("mock recent logs", result.stderr)

    def test_delayed_controller_and_custom_directories(self):
        result = self.run_installer(CURL_DELAY=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Controller 连接正常", result.stdout)
        calls = self.calls()
        self.assertLess(calls.index("build\n"), calls.index("install kernel\n"))
        self.assertLess(calls.index("--user restart"), calls.index("curl"))
        self.assertIn(str(self.config.parent.parent / "systemd/user/mihomo-tui.service"), calls)
        settings = json.loads(self.settings.read_text(encoding="utf-8"))
        self.assertEqual(settings["config_path"], str(self.config))
        self.assertEqual(settings["binary"], str(self.root / "user home/.local/bin/mihomo"))

    def test_repeat_install_keeps_secret_and_original_backup(self):
        original = "rules: [MATCH,DIRECT]\n"
        self.config.write_text(original, encoding="utf-8")
        first = self.run_installer("--skip-service", "--skip-build")
        self.assertEqual(first.returncode, 0, first.stderr)
        secret = yaml.safe_load(self.config.read_text(encoding="utf-8"))["secret"]
        second = self.run_installer("--skip-service", "--skip-build")
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(yaml.safe_load(self.config.read_text(encoding="utf-8"))["secret"], secret)
        self.assertEqual(Path(str(self.config) + ".bak").read_text(encoding="utf-8"), original)

    def test_missing_option_values(self):
        for option in ("--mihomo-version", "--mirror", "--mirror=", "--mihomo-version="):
            result = subprocess.run(["bash", str(ROOT / "install.sh"), option], text=True, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("缺少", result.stderr)

    def test_version_lookup_has_timeouts_and_explicit_version_skips_network(self):
        script = r'''
source "$TEST_ROOT/install.sh"
MIHOMO_VERSION=""
curl() {
  printf '%s\n' "$*" >> "$TEST_LOG"
  printf 'https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.32'
}
resolve_version
printf 'resolved=%s\n' "$MIHOMO_VERSION"
curl() { die 'explicit version must not query the network'; }
resolve_version
'''
        result = subprocess.run(["bash", "-c", script], env={**os.environ, "TEST_ROOT": str(ROOT), "TEST_LOG": str(self.log)}, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("resolved=v1.19.32", result.stdout)
        self.assertIn("--connect-timeout 10 --max-time 30", self.calls())


if __name__ == "__main__":
    unittest.main()
