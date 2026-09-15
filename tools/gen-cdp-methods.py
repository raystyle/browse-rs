#!/usr/bin/env python3
"""从 browser_protocol.json + js_protocol.json 生成 CDP 命令清单。

对齐 browser-harness-js sdk/gen.ts 的口径：跳过 events，收录
experimental/deprecated，跳过 redirect 别名（commands 带 redirect 字段者）。

用法：uv run tools/gen-cdp-methods.py <sdk目录> > crates/cdp/src/methods.txt
清单源头通常是 refs/browser-harness-js/sdk/（研究蓝本，不入库）。
产物 methods.txt 入库；改动协议版本后重跑再提交。
"""
import json
import sys
from pathlib import Path


def collect(path: Path) -> list[str]:
    data = json.loads(path.read_text(encoding="utf-8"))
    out = []
    for domain_spec in data.get("domains", []):
        domain = domain_spec["domain"]
        for command in domain_spec.get("commands", []):
            if command.get("redirect"):
                continue
            out.append(f"{domain}.{command['name']}")
    return out


def main() -> None:
    sdk = Path(sys.argv[1])
    methods: set[str] = set()
    for fname in ("browser_protocol.json", "js_protocol.json"):
        methods.update(collect(sdk / fname))
    for m in sorted(methods):
        print(m)


if __name__ == "__main__":
    main()
