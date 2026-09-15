# ADR-0006：多实例——BROWSE_NAME 命名空间

- 状态：已接受
- 日期：2026-09-15
- 关联：[ADR-0001](0001-daemon-persistent.md)（常驻 daemon）、[ADR-0003](0003-attach-first-spawn-fallback.md)（引擎 profile）

## 背景

agent 并行跑多份浏览任务（例：一个查资料、一个开隔离 profile 测登录态），
单一 daemon（9880）与单一 `~/.browse-rs` 状态目录是硬瓶颈：引擎 profile
被 chrome 单实例锁独占，两个任务会互相连坐。上游 browser-harness 用
`BH_NAME` 解决同型问题。

## 决策

`BROWSE_NAME=<name>` 一个名字同时决定两件事（`crates/browse-core/src/paths.rs`）：

1. **状态目录**：`%USERPROFILE%\.browse-rs\<name>\`——daemon 日志、drops、
   screenshots、录制目录、engine-profile 全部随之隔离。命名实例可各自
   spawn chrome（profile 独占、不互锁）。
2. **daemon 端口**：`9900 + fnv1a(name) % 100`（9900..=9999）。同名恒同口
   （重启不变，CLI 自动拉起能找回）；显式 `BROWSE_PORT` 永远优先。

CLI 子进程继承环境变量，`BROWSE_NAME=work browse up` 与
`BROWSE_NAME=work browse '…'` 落到同一 daemon；不同名字互不可见
（端口与目录都不同）。`/health` 与 `browse status` 带 `name` 字段。

## 后果

- 默认实例零变化（无 BROWSE_NAME 即旧行为：9880、`~/.browse-rs`）。
- 每实例一个 daemon、一条 CDP 会话、一份 vars 表——实例间不共享状态，
  这是特性不是缺陷。
- 坏面：哈希派生可能撞口（~1% 量级），撞上时第二个 daemon 起不来，
  逃生口是显式 `BROWSE_PORT`；同名起两个 daemon 不可能（口被占即发现）。

## 替代方案

- 端口文件（状态目录里写 `port` 文件）：找回更稳但多一次文件协调，
  且首次拉起前目录不存在需要引导逻辑——为罕见的多实例场景不值。
- CLI `--name` 旗标：等价但每次调用都要带；环境变量对 agent 会话
  （export 一次）更顺手，旗标仍可通过 shell 包装实现。
