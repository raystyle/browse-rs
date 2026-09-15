# browse-cli::client

daemon 客户端：本地 HTTP 调用 + 首次使用自动拉起 detached daemon。

## Functions

- `daemon_alive` — daemon 是否在跑（GET /health 通即为在）。
- `daemon_bind` — daemon 的 `host:port` 绑定串（多实例：`BROWSE_NAME` 派生端口，见 ADR-0006）。
- `engine_up` — POST /engine/up（显式起引擎）。
- `ensure_daemon` — 确保 daemon 在跑：不通就 detached 拉起 `browse --serve`，再探活。
- `eval` — POST /eval。返回片段求值结果；daemon 侧错误进 `Err`（错误串已是给人/agent 的下一步指令形态）。
- `health` — GET /health。
- `quit` — POST /quit（退 daemon；daemon 侧顺带只终结自起引擎）。
- `state_dir` — daemon 日志与运行面目录：`%USERPROFILE%\.browse-rs[\<name>]`（多实例）。

## Constants

- `DEFAULT_PORT` — daemon 缺省端口（`BROWSE_PORT` / `BROWSE_NAME` 派生可覆盖）。避开 bh 的 9876。

