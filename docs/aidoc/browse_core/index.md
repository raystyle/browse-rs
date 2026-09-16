# browse-core 0.2.1

browse CLI 的核心库：方言宿主、引擎策略、daemon HTTP API。

三块职责：

- [`parser`]：browser-harness-js 片段方言的手写语法分析器（纯函数，可单测）。
  支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、`return`、
  `//` 注释。不支持：函数字面量、`if/for`、模板字符串；页面逻辑放进
  `Runtime.evaluate` 的 `expression` 字符串里。
- [`js_host`]：方言求值器。`session.<Domain>.<method>(params)` 转发为 CDP
  字符串调用，宿主全局 `listPageTargets` / `resolveWsUrl` /
  `detectBrowsers` / `print`，session 方法族含 `close` / `setActiveSession` /
  `peekEvents`（非破坏事件窥视）。
- [`engine`]：引擎策略（附着优先，缺则自起 clean-chrome 专属实例，只杀
  自己 spawn 的）与引擎状态。
- [`record`]：录制，`Page.startScreencast` 帧流由泵任务落盘
  （`recordStart` / `recordStop`）。
- [`paths`]：实例命名空间（`BROWSE_NAME` -> 状态目录与 daemon 端口，
  多实例的落点，ADR-0006）。
- [`server`]：daemon 的 HTTP API（POST /eval、GET /health、POST /engine/up、
  POST /quit），常驻会话与全局变量跨 CLI 调用保持。

# Examples

解析一条片段（纯语法，不碰网络）：

```
use browse_core::parser::{parse_script, render};

let stmts = parse_script("const tabs = await listPageTargets()").unwrap();
assert_eq!(render(&stmts), "const tabs = await listPageTargets()");
```

## Modules

- [`chrome_mgr`](chrome_mgr.md): 内嵌 Chromium 版本管理器（ADR-0007）：各版本 clean-chrome 在本仓应用
- [`engine`](engine.md): 引擎策略：附着优先，缺则自起（ADR-0003）。
- [`js_host`](js_host.md): 方言求值器：把 [`crate::parser`] 的语句树跑在宿主侧。
- [`parser`](parser.md): browser-harness-js 片段方言的语法分析器（纯函数）。
- [`paths`](paths.md): 实例命名空间（多实例，ADR-0006）：`BROWSE_NAME` 一个名字同时决定
- [`record`](record.md): 录制：`Page.startScreencast` 帧流落盘（方言无回调，泵任务代收）。
- [`semantic`](semantic.md): 语义层近期面：tab 族、交互三件、等待判官。
- [`server`](server.md): daemon 的 HTTP API：常驻会话 + 方言求值 + 引擎生命周期。
- [`surface`](surface.md): 命令面目录（incur-rs 原则的方言版适配）：CLI 子命令、方言全局函数、

