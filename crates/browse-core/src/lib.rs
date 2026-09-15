//! browse CLI 的核心库：方言宿主、引擎策略、daemon HTTP API。
//!
//! 三块职责：
//!
//! - [`parser`]：browser-harness-js 片段方言的手写语法分析器（纯函数，可单测）。
//!   支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、`return`、
//!   `//` 注释。不支持：函数字面量、`if/for`、模板字符串——页面逻辑放进
//!   `Runtime.evaluate` 的 `expression` 字符串里。
//! - [`js_host`]：方言求值器。`session.<Domain>.<method>(params)` 转发为 CDP
//!   字符串调用，宿主全局 `listPageTargets` / `resolveWsUrl` /
//!   `detectBrowsers` / `print`，session 方法族含 `close` / `setActiveSession` /
//!   `peekEvents`（非破坏事件窥视）。
//! - [`engine`]：引擎策略（附着优先，缺则自起 clean-chrome 专属实例，只杀
//!   自己 spawn 的）与引擎状态。
//! - [`server`]：daemon 的 HTTP API（POST /eval、GET /health、POST /engine/up、
//!   POST /quit），常驻会话与全局变量跨 CLI 调用保持。
//!
//! # Examples
//!
//! 解析一条片段（纯语法，不碰网络）：
//!
//! ```
//! use browse_core::parser::{parse_script, render};
//!
//! let stmts = parse_script("const tabs = await listPageTargets()").unwrap();
//! assert_eq!(render(&stmts), "const tabs = await listPageTargets()");
//! ```

pub mod engine;
pub mod js_host;
pub mod parser;
pub mod semantic;
pub mod server;

pub use engine::{Engine, EngineSource, EngineSpec};
pub use js_host::{JsHost, render_result};
pub use parser::snippet_complete;
