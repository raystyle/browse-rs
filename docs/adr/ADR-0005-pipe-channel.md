---
id: ADR-0005
status: accepted
note: 2026-09-15 补：POSIX fd 3/4 布线落地
date: 2026-09-14（POSIX 补记 2026-09-15）
deciders: 用户指示（"spawn 可用改进 支持管道模式"）+ 维护者
---

# ADR-0005：spawn 管道通道（clean-chrome S005）

## Context

clean-chrome 2026-09-14 落地 D02-6/S005：`CLEAN_CHROME_DEBUG=port|pipe|both`
三态。管道契约（devtools_agent_host_impl.cc AdoptPipes）：启动器建两条
匿名管道，Windows 经 `--remote-debugging-io-pipes=<读句柄>,<写句柄>`
传十进制句柄值（POSIX 是 fd 3/4）；协议 ASCIIZ（NUL 分隔 JSON-RPC）；
断管即关浏览器（CloseBrowserSoon）；管道路径不经 WS accept，
DevTools 连接对话框对该通道根本不存在。启动器范本 `tools/pipe-smoke.py`。

Rust 侧约束：std 的匿名管道（`std::io::pipe`）至 1.98 未稳定，
Windows 需要 `CreatePipe` 薄封装（cdp::pipe）+ `SetHandleInformation`
标可继承；POSIX 用 `pipe(2)` + `Command::pre_exec` 里 `dup2` 把两端布到
固定 fd 3/4（pre_exec 里只用 async-signal-safe 的 dup2/close），
`--remote-debugging-io-pipes=3,4` 按契约传字面 fd 号。

## Decision

spawn 支持 `--pipe`（CLI 旗标贯通 `EngineSpec::Auto { pipe }` 与
`/engine/up`）：建两条匿名管道、只把子侧两端标可继承、
`CLEAN_CHROME_DEBUG=pipe`（不开 9222）、句柄值进命令行。`Session` 双通道化：
WS 与管道共用 pending/事件/守卫/sessionId 路由，仅字节泵不同
（`open_ws` / `connect_pipes`，后者经 blocking 线程池）。管道态免端口文件
探测（首条调用即等待就绪），`EngineSource::Spawned.channel` 记录通道。
2026-09-15 补记：POSIX 布线按同一契约落地（`libc` 仅 unix 目标依赖），
编译与管道单测由 CI 的 ubuntu 作业门禁（`cargo clippy/test -p cdp`）。
2026-09-15 晚再补：**真机 macOS 与 Linux 端到端均已验**（lan-mac 与
lan-ubuntu 各自的 out\Release 50 锚构建上 `--remote-debugging-io-pipes=3,4`
双通道 e2e 全绿 exit 0，含录制帧流，零残留）。顺带修掉两处生命周期
耦合：管道泵改游离 std 线程（阻塞读不再拖死 runtime 销毁）；引擎
chrome stdio 全显式（stderr 落 `<state>/engine.log`，不继承调用方
句柄，调用方管道不被挟持 EOF）。因为断管自关已经把浏览器生命周期
绑定到 daemon，所以管道态同时消灭了孤儿浏览器问题。

## Consequences

- 好：零 TCP 面（9222 无监听，实证过）；就绪免轮询；daemon 退出即浏览器退出。
- 好：std 管道未稳定不构成阻塞（自封装约 150 行，句柄/fd 存 usize 天然 Send）。
- 好：双平台同契约（Windows 句柄继承 / POSIX dup2 fd 3/4），Session 侧零分叉。
- 坏：POSIX 通道本机未端到端验证（CI ubuntu 编译+单测门禁，真机待补）。
- 坏：依赖 clean-chrome 47 锚产物（2026-09-14 后构建）；上游 Chromium 没有此变量。
- 坏：句柄/fd 值经命令行传递，任务管理器/日志里可见（本机自动化场景无敏感面）。

## Alternatives

- 只保留端口态：放弃零 TCP 面与生命周期绑定红利，用户已指示要做。
- `both` 双通道默认：调试友好，但 spawn 场景用不上端口，默认 pipe 足矣
  （`PipeMode::Both` 已建模，未来可暴露）。
