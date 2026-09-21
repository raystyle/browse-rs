//! browse CLI 的核心库：方言宿主、引擎策略、daemon HTTP API。
//!
//! 三块职责：
//!
//! - [`parser`]：browser-harness-js 片段方言的手写语法分析器（纯函数，可单测）。
//!   支持：字面量、对象、数组、成员、下标、`await`、`const/let/var`、`return`、
//!   `//` 注释、反引号模板字符串（raw 语义，#18）。不支持：函数字面量、
//!   `if/for`；页面逻辑放进 `Runtime.evaluate` 的 `expression` 字符串里。
//! - [`js_host`]：方言求值器。`session.<Domain>.<method>(params)` 转发为 CDP
//!   字符串调用，宿主全局 `listPageTargets` / `resolveWsUrl` /
//!   `detectBrowsers` / `print`，session 方法族含 `close` / `setActiveSession` /
//!   `peekEvents`（非破坏事件窥视）。
//! - [`engine`]：引擎策略（附着优先，缺则自起 clean-chrome 专属实例，只杀
//!   自己 spawn 的）与引擎状态。
//! - [`record`]：录制，`Page.startScreencast` 帧流由泵任务落盘
//!   （`recordStart` / `recordStop`）。
//! - [`paths`]：实例命名空间（`BROWSE_NAME` -> 状态目录与 daemon 端口，
//!   多实例的落点，ADR-0006）。
//! - [`server`]：daemon 的 HTTP API（POST /eval、GET /health、POST /engine/up、
//!   POST /quit），常驻会话与全局变量跨 CLI 调用保持。
//! - [`surface`]：命令面目录（CLI/全局函数/session 方法三类的单一真相源，
//!   docs/surface 投影由它派生）。
//! - [`workspace`]：workspace 单仓管理（git clone/pull 与仓内文件读取，
//!   #50/#51 配套）。
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

pub mod chrome_mgr;
pub mod cookie_clone;
pub mod engine;
pub mod js_host;
pub mod parser;
pub mod paths;
pub mod record;
pub mod self_update;
pub mod semantic;
pub mod server;
pub mod surface;
pub mod workspace;

pub use engine::{Engine, EngineSource, EngineSpec};
pub use js_host::{JsHost, load_secrets, render_result};
pub use parser::snippet_complete;
pub use surface::CmdKind;
