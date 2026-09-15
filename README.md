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
4. `--pipe`：spawn 走 CDP 管道通道（`CLEAN_CHROME_DEBUG=pipe`，clean-chrome S005 契约），零 TCP 面、断管即关浏览器（ADR-0005；Windows 句柄继承与 POSIX fd 3/4 布线双实现，POSIX 侧由 CI ubuntu 门禁）。

安全守卫（程序级强制，`Session::call` 层，错误一律带 CTA「下一步」）：
`Browser.close` / `Browser.setWindowBounds` 一律拒绝；`Target.closeTarget`
只放行自建 tab：`listPageTargets()` / `currentTab()` 带 `own` 字段标注
哪些能关（chrome 启动初始页与用户 tab 恒 `own:false`，用 `switchTab` 切走）。

语义层近期面（对齐 harness(py) 高频操作，坑表教训落地）：
`newTab(url?)`（先 about:blank 再 goto，回 `{targetId,title,url,own}`）、
`switchTab(id)`、`currentTab()`、`closeTab(id?)`（守卫同 `Target.closeTarget`）、
`clickAt(x,y)`（trusted 鼠标事件）、`fillInput(sel,text)`（SelectAll 不发
Ctrl+A、回读严格验证）、`pressKey(key)`、`waitLoad(ms?)`、`waitIdle(ms?)`
（network 静默窗口）。Input 派发撞后台 tab 挂起时自动 activate 自愈重试一次。

元素引用（D35-lite + 主动代际失效）：`snapshot()` 给每个带 backendNodeId
的节点盖短 `ref`（e1、e2…），`clickRef(ref)`（滚动可见->量中心->trusted
点击）与 `fillRef(ref,text)`（objectId 上 focus->SelectAll+insertText->
同节点回读严格验证）按 ref 操作：选择器会随页面重构漂移，
backendNodeId 不会。引用表只保留最近一次 snapshot（整表替换）；
snapshot 时在页窗口盖 `__browse_ref_gen` 代标记，引用前核对：
文档被导航重开即整表作废（SPA 同文档 pushState 不误伤），
另有 `DOM.resolveNode`/零尺寸被动兜底，错误一律带「重新 snapshot」CTA，
绝不静默点错位置。

录制：`recordStart(opts?)` / `recordStop()`。`Page.startScreencast` 帧流
由常驻泵任务落盘 `%USERPROFILE%\.browse-rs\record-<ts>\frame-NNNNNN.png`
（opts 可 `everyNthFrame` 源端抽帧、`maxWidth`/`maxHeight` 限宽高，
轻量剪辑面），stop 回 `{frames,bytes,dir}`。ack 按帧自带 sessionId
路由；帧走事件缓冲（上限 1000），录短段、要完整事件流先 peek。

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

环境变量：`BROWSE_PORT`（daemon 端口，默认 9880）、`BROWSE_NAME`（命名实例，
ADR-0006：状态目录与派生端口 9900-9999 全隔离，`BROWSE_NAME=work browse up`
即起一个与默认实例并行的引擎）、`BROWSE_CHROME`（chrome.exe 路径）、
`BROWSE_CDP_WS`（钉死连接）、`BROWSE_NO_ATTACH=1`（跳过附着探测强制 spawn）、
`BROWSE_EVAL_TIMEOUT`（秒，默认 300）。

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
`cdpMethods(domain?)`（652 命令运行时探针）/ `snapshot()`（AX 树快照，
`{url,title,nodes:[{id,role,name,value,checked,backendNodeId,...}]}`，
对齐 browser-use-pi）/ `screenshot(path?, full?)`（存 PNG 回 `{path,bytes}`）/
`print(x)`。session 方法族：`connect`（支持 `timeoutMs`）/ `use` / `close`
（断开可重连）/ `setActiveSession` / `waitFor` / `waitJs(expression, ms?)`
（页内谓词轮询，返回真值本身，pi `page.waitFor` 的方言代偿）/ `call` /
`isConnected` / `getActiveSession`。事件家族：`peekEvents(method, n?)`
（非破坏窥视）、`peekEventsSince(method, sinceSeq, n?)`（seq 游标增量）、
`findEvents(method, "params.requestId", <值>, n?)`（等值过滤）。事件进缓冲
即盖单调 `seq`。方法拼错时 CDP `not found` 错误自动附相近建议
（清单 652 条由 `tools/gen-cdp-methods.py` 生成，`crates/cdp/src/methods.txt`）。
clean-chrome S006（50 锚）起 `Runtime.consoleAPICalled` 的 args 与
`Runtime.exceptionThrown` 的 exception 不带 preview（objectId 保留），
要对象细节走 `objectId + Runtime.getProperties`；`Runtime.evaluate`
的 preview 两态不受影响。

域策略（对齐 pi policy）：`BROWSE_DENY_DOMAINS` / `BROWSE_ALLOW_DOMAINS`
（逗号分隔，后缀匹配含子域；deny 优先）在 `Page.navigate` /
`Target.createTarget` 上程序级拦截，未配置不拦。

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
BROWSE_E2E=1 BROWSE_NO_ATTACH=1 cargo test -p browse-core --test e2e   # 真 chrome 端到端（需本机 clean-chrome；NO_ATTACH 防误附着用户浏览器）
cargo clippy --workspace --all-targets -- -D warnings
```

CI 两个作业：windows-latest 跑全量门禁（fmt/clippy/test/doc/aidoc），
ubuntu-latest 编译并单测 cdp（POSIX 管道 fd 3/4 布线的平台门禁）。

详读 `AGENTS.md`（命令与门禁）、`docs/architecture.md`（现在怎么拼）、`docs/adr/`（为什么）。

## Roadmap（v0.1 之外）

- ~~元素引用 D35-lite（snapshot 短 ref + clickRef/fillRef，backendNodeId 锚）~~ 已落地
- ~~录制（Page.startScreencast 帧流）~~ 已落地（源端抽帧/限宽高当轻量剪辑）
- ~~元素引用的进阶（主动代际失效）~~ 已落地（`__browse_ref_gen` 窗口代标记，SPA 不误伤）
- ~~多实例（bh `BH_NAME` 式）~~ 已落地（BROWSE_NAME，ADR-0006）
- ~~POSIX 管道通道（fd 3/4 布线）~~ 已落地（真机 Windows/macOS/Linux 三平台端到端全绿 + CI ubuntu 门禁）

大值保护（artifact/checkpoint 的降级实现，已落地）：片段结果序列化超
32KB 时自动落盘 `%USERPROFILE%\.browse-rs\drops\value-<ts>.{json,txt}`，
stdout 只回 `{"__dropped":true,"bytes":N,"path":"...","preview":"前 160 字符"}`
（防大 JSON 淹没 agent 上下文；daemon 与 vars 表不受影响）

## 明确不做（用户裁定，勿再提议）

- MCP server（接口面就是 CLI + 方言）
- 秘密脱敏（BROWSE_REDACT 输出层掩码）
- 模型循环/观察循环（消费者是编码 agent，它自带循环与视觉）
- 控制流/函数进方言（ADR-0002）
- Cloud browser（与 clean-chrome 本地优先哲学相反）

[browser-harness-rs]: https://github.com/browser-use/browser-harness-js
