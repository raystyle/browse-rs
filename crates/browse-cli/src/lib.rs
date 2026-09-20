//! browse CLI 的可测半边：daemon 客户端（HTTP 调用 + 自动拉起）。
//!
//! bin 侧（`main.rs`）只做接线：手写参数解析、求值形态（`--eval` 显式 /
//! stdin 管道批处理 / `--repl` 显式交互；裸调用出本仓帮助体）
//! 与 up/down/status 生命周期子命令。
//! 会话状态（方言变量、活动 tab、引擎）全在 daemon 侧持久，
//! CLI 进程本身无状态、即起即走。

pub mod client;
pub mod fetch;
pub mod ledger;
pub mod render;

/// 环境敏感测试的进程级互斥锁（评审 F1）：ledger 测试会 set/remove 进程级
/// env（`BROWSE_NAME`/`BROWSE_LEDGER_PRIVATE_KEY`），同二进制并行跑会互
/// 踩出「公钥不配对」类假故障；env 敏感测试开头各取此锁（hst TEST_ENV_LOCK
/// 同款）。
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
