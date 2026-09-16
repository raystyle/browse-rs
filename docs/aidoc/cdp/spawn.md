# cdp::spawn

找到 Chrome 可执行文件并拉起一个带调试口的专属实例。

只服务于「引擎自起」路径：独立 user-data 目录、调试端口自动分配
（`--remote-debugging-port=0`，真实端口写进 profile 的
`DevToolsActivePort`）、`browse down` 时才终结，绝不碰用户默认 profile。

## Functions

- `chrome_binary_in_dir` — 在部署目录里解析 chrome 可执行文件（布局感知）：Windows `chrome.exe`、
- `chrome_binary_name` — 返回本平台的 chrome 二进制名（Windows `chrome.exe`，其余 `chrome`）。
- `find_chrome` — 依优先序探测 Chrome 可执行文件，找不到返回 `None`。
- `spawn_engine` — 拉起一个带调试口的专属引擎实例（独立 profile、端口自动分配）。
- `spawn_engine_pipes` — 按管道契约拉起 clean-chrome（S005 / D02-6；Windows 句柄态与 POSIX
- `terminate_pid` — 强杀进程树（优雅退出 `Browser.close` 失败后的兜底）。
- `wait_devtools_ready` — 等 spawn 出来的实例调试口就绪，返回 WS URL（读它 profile 下的

## Types

- `PipeEngine` — 管道态引擎的句柄：chrome 子进程加留在启动器侧的两条 CDP 管道端。
- `PipeMode` — 管道通道的开关取值，即 `CLEAN_CHROME_DEBUG` 环境变量的值域。

