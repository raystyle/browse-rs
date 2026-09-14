# cdp::spawn

找到 Chrome 可执行文件并拉起一个带调试口的专属实例。

只服务于「引擎自起」路径：独立 user-data 目录、调试端口自动分配
（`--remote-debugging-port=0`，真实端口写进 profile 的
`DevToolsActivePort`）、`browse down` 时才终结，绝不碰用户默认 profile。

## Functions

- `find_chrome` — 依优先序找 Chrome 可执行文件：`BROWSE_CHROME` 环境变量 ->
- `spawn_engine` — 拉起专属引擎实例。参数：可执行文件、独立 profile 目录、是否无头。
- `spawn_engine_pipes` — 按管道契约拉起 clean-chrome（S005 / D02-6，当前仅 Windows）。
- `terminate_pid` — 强杀进程树（优雅退出 `Browser.close` 失败后的兜底）。
- `wait_devtools_ready` — 等 spawn 出来的实例调试口就绪（读它 profile 下的 `DevToolsActivePort`），

## Types

- `PipeEngine` — 管道态引擎句柄：子进程 + 留在启动器侧的两条 CDP 管道端。
- `PipeMode` — 管道通道三态：CLEAN_CHROME_DEBUG 的取值。

