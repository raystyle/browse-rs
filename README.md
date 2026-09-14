# browse-rs

给 agent 用的 browse CLI（Rust）：JS 方言片段驱动 [clean-chrome](https://github.com/raystyle/clean-chrome)（自编 Chromium，`--auto-allow-devtools-connections` 免确认对话框）。以 [browser-harness-rs] 为样例忠实移植，按 rust-code-as-docs 规范组织（`///` + doctest 锁公开用法，ADR 锁 why）。

```
browse '<方言片段>' ──HTTP POST /eval──> daemon（browse.exe --serve，127.0.0.1:9880）
   │                                       ├─ Session：CDP 通道（WS 或 S005 管道）+ flatten attach
   │                                       ├─ JsHost：方言解释器（vars 跨调用持久）
   │                                       └─ Engine：附着探测 -> spawn 兜底（只杀自己的）
   └─ 首次使用自动拉起 daemon（detached），browse down 退出
```

## 引擎策略（ADR-0003）

1. 显式 `--ws` / `--port` / `BROWSE_CDP_WS` 直连。
2. 否则探测本机已开调试口（`/json/version`@9222 -> 默认 profile 的 `DevToolsActivePort`），命中即附着（绝不关用户的浏览器）。
3. 都没有就 spawn 专属实例：独立 profile、`--no-sandbox`、可 `--headless`。
4. `--pipe`：spawn 走 CDP 管道通道（`CLEAN_CHROME_DEBUG=pipe`，clean-chrome S005 契约），零 TCP 面、断管即关浏览器（ADR-0005，Windows 先行）。

安全守卫（程序级强制，`Session::call` 层）：`Browser.close` / `Browser.setWindowBounds` 一律拒绝；`Target.closeTarget` 只放行自建 tab。

## 命令

```bash
browse '<片段>'                     # 求值（自动拉 daemon 与引擎）
browse -e '<片段>' | stdin | TTY    # 其余两形态（括号配平批处理 / rustyline REPL）
browse --new-tab '<片段>'           # 先开 about:blank 再求值
browse --connect <ws|端口> '<片段>'  # 显式附着
browse up [--headless] [--pipe] [--chrome <path>] [--ws <url> | --port <p>]
browse down                          # 退 daemon；只杀自己 spawn 的实例
browse status [--json]
browse --serve [--bind host:port]    # 前台跑 daemon
```

环境变量：`BROWSE_PORT`（daemon 端口，默认 9880）、`BROWSE_CHROME`（chrome.exe 路径）、`BROWSE_CDP_WS`（钉死连接）、`BROWSE_EVAL_TIMEOUT`（秒，默认 300）。

退出码：`0` 成功 / `1` 执行失败 / `2` 用法错；错误串形态 `browse: <下一步指令>` 进 stderr。
方言错误一律 CTA 三段式：`诊断（行L:列C）；下一步：<可照抄的写法或命令>`
（如 `未定义变量 tabs；下一步：先在前一条片段里 const tabs = <值>`），
由 `tests/dialect_errors.rs` 契约锁定。

## 片段方言（与 browser-harness-js 对齐）

```js
await session.connect({port:9222})
const tabs = await listPageTargets()
await session.use(tabs[0].targetId)
await session.Page.navigate({url:"https://example.com"})
await session.waitFor("Page.loadEventFired", undefined, 15000)
return (await session.Runtime.evaluate({expression:"document.title", returnByValue:true})).result.value
```

支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、`return`、`//` 注释。
不支持：函数字面量、`if/for/while`、模板字符串。报错会提示把页面逻辑放进
`Runtime.evaluate` 的 `expression` 字符串（页内是真 V8）。
`session.<Domain>.<method>(params)` 直转 CDP 字符串调用，652 个方法无封装。
宿主全局：`listPageTargets()` / `resolveWsUrl(opts?)` / `detectBrowsers()` /
`print(x)`。session 方法族：`connect`（支持 `timeoutMs`，等 Allow 给 30000）/
`use` / `close`（断开不关浏览器，可重连）/ `setActiveSession` / `waitFor` /
`call` / `isConnected` / `getActiveSession`。事件家族：`peekEvents(method, n?)`
（非破坏窥视，官方 `onEvent` 的无函数方言等价面）、
`peekEventsSince(method, sinceSeq, n?)`（seq 游标增量轮询，不重看旧事件）、
`findEvents(method, "params.requestId", <值>, n?)`（点分路径等值过滤，
挑特定请求/帧的事件）。事件进缓冲即盖单调 `seq`，waitFor/peek 拿到的
事件自带游标。

## 与样例（browser-harness-rs）的差异

| 点 | 样例 | 本仓库 |
|---|---|---|
| daemon | `--serve` 手动起，一次性模式每进程重连 | 首次使用自动拉起，常驻持久（vars/活动 tab 跨调用） |
| 引擎 | 要求浏览器已在跑 | 附着优先，缺则 spawn clean-chrome |
| CDP 通道 | 仅 WS | WS + 管道（`--pipe`，S005） |
| 守卫 | 无 | `Browser.close` 等拦截，`Target.closeTarget` 只放行自建 tab |
| 事件缓冲 | 无界 Vec | 环形上限 1000 |

## 开发

```bash
cargo test --workspace                    # 单元 + 契约（含 doctest）
BROWSE_E2E=1 cargo test -p browse-core --test e2e   # 真 chrome 端到端（需本机 clean-chrome）
cargo clippy --workspace --all-targets -- -D warnings
```

详读 `AGENTS.md`（命令与门禁）、`docs/architecture.md`（现在怎么拼）、`docs/adr/`（为什么）。

## Roadmap（v0.1 之外）

- 语义层助手（goto_url / click_ref / snapshot_interactives 等 snake_case 面）
- POSIX 管道通道（fd 3/4 布线）
- 多实例（bh `BH_NAME` 式）、看板、应用层

[browser-harness-rs]: https://github.com/browser-use/browser-harness-js
