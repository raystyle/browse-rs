//! browse CLI 的可测半边：daemon 客户端（HTTP 调用 + 自动拉起）。
//!
//! bin 侧（`main.rs`）只做接线：手写参数解析、三传输形态
//! （`--eval` / stdin 括号配平 / TTY REPL）与 up/down/status 生命周期子命令。
//! 会话状态（方言变量、活动 tab、引擎）全在 daemon 侧持久，
//! CLI 进程本身无状态、即起即走。

pub mod client;
