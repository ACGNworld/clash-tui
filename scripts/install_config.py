#!/usr/bin/env python3
"""Prepare installer configuration before changing the running installation."""

import argparse
import json
from pathlib import Path
import secrets
import sys
from urllib.parse import urlsplit, urlunsplit

import yaml


def patch_config(original, node, updates):
    """Use YAML token positions to retain comments and TUI markers."""
    tokens = list(yaml.scan(original))
    edits = []
    additions = dict(updates)
    pairs = node.value if node is not None else []
    for index, (key, _) in enumerate(pairs):
        if key.value not in updates:
            continue
        end = pairs[index + 1][0].start_mark.index if index + 1 < len(pairs) else node.end_mark.index
        field_tokens = [token for token in tokens if key.end_mark.index <= token.start_mark.index < end]
        colon = next((token for token in field_tokens if isinstance(token, yaml.tokens.ValueToken)), None)
        if colon is None:
            raise ValueError("请将 external-controller 写成 key: value 形式后重试")
        value_tokens = [token for token in field_tokens if isinstance(token, (yaml.tokens.ScalarToken, yaml.tokens.AliasToken))]
        if value_tokens:
            start, stop = value_tokens[0].start_mark.index, value_tokens[0].end_mark.index
        else:
            anchors = [token for token in field_tokens if isinstance(token, yaml.tokens.AnchorToken)]
            start = stop = anchors[-1].end_mark.index if anchors else colon.end_mark.index
        edits.append((start, stop, " " + json.dumps(additions.pop(key.value), ensure_ascii=False) + " "))
        # A repaired value is a string, even if the old value had an explicit tag.
        edits.extend((token.start_mark.index, token.end_mark.index, "") for token in field_tokens if isinstance(token, yaml.tokens.TagToken))

    if additions:
        fields = [f"{key}: {json.dumps(value, ensure_ascii=False)}" for key, value in additions.items()]
        if node is not None and node.flow_style:
            position = node.end_mark.index - 1  # before the closing }
            separator = ", " if pairs and not original[:position].rstrip().endswith(",") else " "
            text = separator + ", ".join(fields)
        else:
            position = next((token.start_mark.index for token in tokens if isinstance(token, yaml.tokens.DocumentEndToken)), len(original))
            text = ("" if original[:position].endswith("\n") or position == 0 else "\n") + "\n".join(fields) + "\n"
        edits.append((position, position, text))
    # Apply appended fields first if an empty value at EOF shares their position.
    for start, stop, text in sorted(reversed(edits), key=lambda edit: edit[:2], reverse=True):
        original = original[:start] + text + original[stop:]
    return original


def controller_url(controller):
    url = controller if "://" in controller else "http://" + controller
    # Mihomo also accepts an empty listening host, such as :9090.
    if url.startswith("http://:"):
        url = url.replace("http://:", "http://127.0.0.1:", 1)
    parsed = urlsplit(url)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or not parsed.port:
        raise ValueError("external-controller 必须是有效的 host:port 地址")
    if parsed.hostname in ("0.0.0.0", "::"):
        parsed = parsed._replace(netloc=f"127.0.0.1:{parsed.port}")
    return urlunsplit(parsed)


def prepare(config_path, settings_path, output, controller, service, binary):
    original = config_path.read_text(encoding="utf-8") if config_path.exists() else None
    node = None
    if original is None:
        config = {
            "mixed-port": 7890,
            "allow-lan": False,
            "mode": "rule",
            "log-level": "info",
            "dns": {
                "enable": True,
                "ipv6": True,
                "enhanced-mode": "fake-ip",
                "nameserver": ["system"],
            },
            "rules": ["MATCH,DIRECT"],
        }
    else:
        config = yaml.safe_load(original)
        if config is None:
            config = {}
        if not isinstance(config, dict):
            raise ValueError("Mihomo 配置顶层必须是 YAML 映射")
        node = yaml.compose(original)
        if isinstance(node, yaml.ScalarNode) and node.tag == "tag:yaml.org,2002:null" and not node.value:
            node = None  # an otherwise empty document with a --- marker
        if node is not None and not isinstance(node, yaml.MappingNode):
            raise ValueError("Mihomo 配置顶层必须是 YAML 映射")
        if node is not None:
            if any(not isinstance(key, yaml.ScalarNode) for key, _ in node.value):
                raise ValueError("Mihomo 配置顶层的键必须是标量")
            keys = [key.value for key, _ in node.value]
            if len(keys) != len(set(keys)):
                raise ValueError("Mihomo 配置有重复的顶层键，请先修正")

    updates = {}
    if not config.get("external-controller"):
        config["external-controller"] = controller
        updates["external-controller"] = controller
    controller = config["external-controller"]
    if not isinstance(controller, str):
        raise ValueError("external-controller 必须是字符串")
    url = controller_url(controller)
    if "secret" not in config:
        config["secret"] = secrets.token_hex(16)
        updates["secret"] = config["secret"]
    secret = config["secret"]
    if secret is None:
        secret = ""
    if not isinstance(secret, str):
        raise ValueError("secret 必须是字符串，请给数字等值加引号")

    settings = {}
    if settings_path.exists():
        settings = json.loads(settings_path.read_text(encoding="utf-8"))
        if not isinstance(settings, dict):
            raise ValueError("clash-tui 设置顶层必须是 JSON 对象")
    settings.update(
        url=url,
        secret=secret,
        service=service,
        config_path=str(config_path),
        binary=binary,
    )

    output.mkdir(parents=True, exist_ok=True)
    if original is None:
        config_text = yaml.safe_dump(config, allow_unicode=True, sort_keys=False)
    else:
        config_text = patch_config(original, node, updates) if updates else original
        if yaml.safe_load(config_text) != config:
            raise ValueError("无法保留原文件格式并补齐配置，请手动填写 external-controller 和 secret")
    (output / "config.yaml").write_text(config_text, encoding="utf-8")
    (output / "settings.json").write_text(
        json.dumps(settings, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (output / "connection.json").write_text(
        json.dumps({"url": url, "secret": secret}), encoding="utf-8"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--settings", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--controller", required=True)
    parser.add_argument("--service", required=True)
    parser.add_argument("--binary", required=True)
    args = parser.parse_args()
    try:
        prepare(args.config, args.settings, args.output, args.controller, args.service, args.binary)
    except (OSError, ValueError, yaml.YAMLError) as error:
        print(f"配置准备失败：{error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
