# browse-core::server

daemon 的 HTTP API：常驻会话 + 方言求值 + 引擎生命周期。

移植自 browser-harness-rs `src/server.rs`（POST /eval、GET /health、
POST /quit 三端点保留），新增 `/engine/up` 与求值前的懒引擎策略：

- 求值前未连接且片段里没有 `session.connect` -> 自动 [`crate::Engine::ensure`]；
  片段自带 `session.connect` 则不预连（显式意图优先，ADR-0004）。
- 求值撞上「Not connected」再兜底 ensure + 重试一次（覆盖先语句后连接的写法）。
- 单飞槽：同一时刻只跑一条片段，后来者排队（不拒 429）。
- `/eval` 超时默认 300 秒（`BROWSE_EVAL_TIMEOUT`，单位秒）。

## Functions

- `serve` — 起 HTTP daemon，监听 `bind`，直到 POST /quit。

## Types

- `Daemon` — daemon 运行面：宿主、引擎、缺省引擎意图、单飞槽与退出旗标。
- `EngineUpRequest` — `/engine/up` 请求体。
- `EvalRequest` — `/eval` 请求体。

