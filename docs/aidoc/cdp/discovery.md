# cdp::discovery

连接线索到 WebSocket URL 的解析：`wsUrl` / `port` / `profileDir` 三条路。

Chrome 144+ 对默认 profile 的 HTTP `/json` 端点可能不服务，
`DevToolsActivePort` 文件（user-data 目录根下，首行端口、次行 WS 路径）
是可靠兜底。clean-chrome 无参数启动即开 9222，正常走 `/json/version`。

## Functions

- `default_profile_dirs` — clean-chrome / Chromium / Chrome 的默认 user-data 目录（按探测优先序）。
- `http_version_ws_url` — GET `<http>/json/version` 取 `webSocketDebuggerUrl`。
- `parse_port` — 解析端口字面量：`"9222"`、`"http://127.0.0.1:9222"`、`"127.0.0.1:9222/"` 都出 `9222`。
- `probe_default` — 引擎附着探测：返回本机已开调试口浏览器的 WS URL，没有则 `None`。
- `resolve_ws_url` — 解析 [`super::ConnectOptions`] 为 WS URL。`profileDir` 路径会轮询等文件出现
- `wait_active_port_file` — 轮询 profile 目录下的 `DevToolsActivePort`，直到超时。
- `ws_from_active_port_text` — 从 `DevToolsActivePort` 文件文本解析 WS URL：首行端口，次行 `/devtools/...` 路径。

