# browse-core::engine

引擎策略：附着优先，缺则自起（ADR-0003）。

clean-chrome 专为自动化而生（`--auto-allow-devtools-connections` 免确认、
无参数启动即开 9222），所以：

1. 显式 `ws` / `port`（CLI 旗标或 `BROWSE_CDP_WS`）优先直连。
2. 否则探测本机已开的调试口（`/json/version`@9222 -> 默认 profile 的
   `DevToolsActivePort`），命中即附着；人机共存，绝不关用户的浏览器。
3. 都没有就 spawn 专属实例：独立 profile、`--remote-debugging-port=0`、
   可 `--headless`。[`Engine::shutdown`] 只终结自己 spawn 的（优雅
   `Browser.close` -> 兜底杀进程树）。

连上后自动 attach 首个 page target（没有就开 about:blank），
让 agent 一条命令即可 `session.Page.navigate(...)`（ADR-0004）；
片段里的显式 `session.connect` / `session.use` 仍然可覆盖。

## Types

- `Engine` — 引擎状态机：确保连接、报告来源、只终结自己 spawn 的。
- `EngineSource` — 引擎现状的可序列化快照，`/health` 面直接用。
- `EngineSpec` — 引擎指令意图，由 CLI 旗标或环境变量解析而来。

