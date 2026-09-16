---
id: REQ-002
title: browse --llms 发现通道
status: implemented
priority: should
trace: cargo test --workspace 与 browse --llms [--full|--json] 三形态冒烟（diff 对 docs/surface 同源零漂移；9880 零监听证不拉 daemon）
---

# REQ-002：browse --llms 发现通道

## Scenario

agent 在只装了二进制（cargo install 进 PATH）的机器上用 browse，手边没有仓，需要从 CLI 本体拿到命令面（evo-adr:code-kit 的 tool-cli-agents 发现三通道之 --llms 通道：零常驻成本，按需一次）。

## Criteria

- [x] `browse --llms` 打印紧凑命令清单（复用 `surface::render_llms`，与 docs/surface/llms.txt 同源零漂移）
- [x] `browse --llms --full` 或同等形态打印完整清单（`render_llms_full`）
- [x] `browse --llms --json` 打印 JSON Schema 包（`render_schema`）
- [x] 输出走 stdout（可管道），不拉起 daemon
- [x] 命令面登记进 docs/surface 的命令目录（surface_contract 锁漂移）
