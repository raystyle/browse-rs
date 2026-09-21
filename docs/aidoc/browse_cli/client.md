# browse-cli::client

daemon 客户端：本地 HTTP 调用 + 首次使用自动拉起 detached daemon。

## Functions

- `daemon_alive` — 探测 daemon 是否在跑：GET /health 通即为在。
- `daemon_bind` — 返回 daemon 监听的 `host:port` 串；多实例由 `BROWSE_NAME` 派生端口（ADR-0006）。
- `decode_arg_b64` — 解码 `-b/--b64` 通道的片段实参（#22）：标准 base64 解码为 UTF-8
- `engine_up` — POST /engine/up 显式起引擎，走 ensure 全链（附着优先缺则 spawn）。
- `ensure_daemon` — daemon 不在跑时 detached 拉起 `browse --serve` 并探活到通；已在跑则直接返回。
- `ensure_daemon_with_env` — 同 [`ensure_daemon`]，但给新拉起的 daemon 进程注入环境变量
- `eval` — 把方言片段 POST 到 daemon 的 /eval 求值。
- `health` — GET /health 取 daemon 状态面；daemon 不在时报可照抄的拉起提示。
- `quit` — POST /quit 退 daemon，daemon 侧顺带只终结自起引擎（附着来源不动）。
- `skills_passthrough_env` — 技能层透传环境（#50/#51）：拉起新 daemon 时把 CLI 进程里的 workspace
- `state_dir` — 返回 daemon 日志与运行面目录（`%USERPROFILE%\.browse-rs[\<name>]`，多实例各一份）。

## Types

- `UpParams` — `engine_up` 的参数面：CLI up 旗标直通（字段名即 /engine/up 请求键）。

## Constants

- `DEFAULT_PORT` — daemon 的缺省端口，`BROWSE_PORT` 与 `BROWSE_NAME` 派生端口可覆盖。

