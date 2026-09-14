# cdp::session

一条 browser-level WebSocket + flatten attach + sessionId 路由。

移植自 browser-harness-rs `src/session.rs`，差异：
事件缓冲有上限（防长跑泄漏）；[`Session::call`] 内置安全守卫
（拦 `Browser.close` 等破坏性方法、`Target.closeTarget` 只放行自建 tab）。

## Functions

- `is_browser_method` — 方法是否属于 browser 端点域（不附 `sessionId`）。

## Types

- `ConnectOptions` — 连接线索三选一：`wsUrl` / `port` / `profileDir`。
- `PageTarget` — 一个可附着的 page target（已滤 `chrome://`、`devtools://`）。
- `Session` — 常驻 CDP 会话。clone `Arc<Self>` 共享同一条连接。

