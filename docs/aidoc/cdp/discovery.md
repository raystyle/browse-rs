# cdp::discovery

连接线索到 WebSocket URL 的解析：`wsUrl` / `port` / `profileDir` 三条路。

Chrome 144+ 对默认 profile 的 HTTP `/json` 端点可能不服务，
`DevToolsActivePort` 文件（user-data 目录根下，首行端口、次行 WS 路径）
是可靠兜底。clean-chrome 无参数启动即开 9222，正常走 `/json/version`。

## Functions

- `default_profile_dirs` — 返回默认 user-data 目录清单（clean-chrome / Chromium / Chrome，按探测优先序）。
- `detect_browsers` — 扫默认 profile 目录列出所有可附着候选：读各目录的 `DevToolsActivePort`
- `http_version_ws_url` — GET `<http>/json/version` 取 `webSocketDebuggerUrl`（自带 `timeout` 超时）。
- `parse_port` — 端口字面量的宽容解析：`"9222"`、`"http://127.0.0.1:9222"`、`"127.0.0.1:9222/"` 都出 `9222`。
- `probe_default` — 探测本机已开调试口的浏览器，返回其 WS URL；没有则 `None`。
- `resolve_ws_url` — 把 [`super::ConnectOptions`] 三线索解析成 WS URL。
- `wait_active_port_file` — 轮询 profile 目录下的 `DevToolsActivePort`，直到超时。
- `ws_from_active_port_text` — 从 `DevToolsActivePort` 文件文本解析 WS URL：首行端口，次行 `/devtools/...` 路径。

## Types

- `DetectedBrowser` — 可附着浏览器的候选描述，由 [`detect_browsers`] 产出。

