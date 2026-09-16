//! Chrome DevTools Protocol（CDP）连接层：一条常驻 browser-level WebSocket 会话。
//!
//! 移植自 browser-harness-rs 的 `session.rs`，职责单一：
//!
//! - [`Session`]：连 browser 端点（不是某个 tab 的 `/devtools/page/...`），
//!   `Target.attachToTarget({flatten:true})` 后按 `sessionId` 路由非 browser 域方法，
//!   事件进环形缓冲供 [`Session::wait_for`] 消费。
//! - [`discovery`]：把 `wsUrl` / `port` / `profileDir` 三种线索解析成 WebSocket URL
//!   （Chrome 144+ 默认 profile 下 HTTP `/json` 端点可能不服务，`DevToolsActivePort`
//!   文件是兜底真相）。
//! - [`spawn`]：找到并拉起一个 Chrome 可执行文件，等它的 `DevToolsActivePort` 就绪。
//!
//! 本 crate 不做语义封装：没有 `goto()`、没有 `click()`。调用方（`browse-core`）
//! 直接写 `session.call("Page.navigate", params)`。
//!
//! # Examples
//!
//! ```no_run
//! # // no_run：需要本机 9222 开着真浏览器
//! # async fn demo() -> anyhow::Result<()> {
//! use cdp::{ConnectOptions, Session};
//! use serde_json::json;
//!
//! let s = Session::new();
//! s.connect_opts(ConnectOptions { port: Some(9222), ..Default::default() }).await?;
//! let tabs = s.list_page_targets().await?;
//! s.use_target(&tabs[0].target_id).await?;
//! s.call("Page.navigate", json!({ "url": "https://example.com" })).await?;
//! # Ok(())
//! # }
//! ```

pub mod discovery;
pub mod methods;
pub mod pipe;
pub mod session;
pub mod spawn;

pub use discovery::{parse_port, resolve_ws_url, ws_from_active_port_text};
pub use session::{ConnectOptions, PageTarget, Session};
