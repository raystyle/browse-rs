# cdp 0.12.2

Chrome DevTools Protocol（CDP）连接层：一条常驻 browser-level WebSocket 会话。

移植自 browser-harness-rs 的 `session.rs`，职责单一：

- [`Session`]：连 browser 端点（不是某个 tab 的 `/devtools/page/...`），
  `Target.attachToTarget({flatten:true})` 后按 `sessionId` 路由非 browser 域方法，
  事件进环形缓冲供 [`Session::wait_for`] 消费。
- [`discovery`]：把 `wsUrl` / `port` / `profileDir` 三种线索解析成 WebSocket URL
  （Chrome 144+ 默认 profile 下 HTTP `/json` 端点可能不服务，`DevToolsActivePort`
  文件是兜底真相）。
- [`spawn`]：找到并拉起一个 Chrome 可执行文件，等它的 `DevToolsActivePort` 就绪。

本 crate 不做语义封装：没有 `goto()`、没有 `click()`。调用方（`browse-core`）
直接写 `session.call("Page.navigate", params)`。

# Examples

```no_run
# // no_run：需要本机 9222 开着真浏览器
# async fn demo() -> anyhow::Result<()> {
use cdp::{ConnectOptions, Session};
use serde_json::json;

let s = Session::new();
s.connect_opts(ConnectOptions { port: Some(9222), ..Default::default() }).await?;
let tabs = s.list_page_targets().await?;
s.use_target(&tabs[0].target_id).await?;
s.call("Page.navigate", json!({ "url": "https://example.com" })).await?;
# Ok(())
# }
```

## Modules

- [`discovery`](discovery.md): 连接线索到 WebSocket URL 的解析：`wsUrl` / `port` / `profileDir` 三条路。
- [`methods`](methods.md): CDP 命令方法清单与相近建议。
- [`pipe`](pipe.md): 匿名管道薄封装。std 的 anonymous pipe（`std::io::pipe`）至 1.98 仍未稳定，
- [`session`](session.md): 一条 browser-level WebSocket + flatten attach + sessionId 路由。
- [`spawn`](spawn.md): 找到 Chrome 可执行文件并拉起一个带调试口的专属实例。

