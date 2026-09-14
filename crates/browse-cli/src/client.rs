//! daemon 客户端：本地 HTTP 调用 + 首次使用自动拉起 detached daemon。

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// daemon 缺省端口（`BROWSE_PORT` 可覆盖）。避开 bh 的 9876。
pub const DEFAULT_PORT: u16 = 9880;

/// daemon 的 `host:port` 绑定串。
///
/// # Examples
///
/// ```
/// let port = std::env::var("BROWSE_PORT").unwrap_or_else(|_| "9880".into());
/// assert_eq!(browse_cli::client::daemon_bind(), format!("127.0.0.1:{port}"));
/// ```
pub fn daemon_bind() -> String {
    let port = std::env::var("BROWSE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    format!("127.0.0.1:{port}")
}

fn http() -> String {
    format!("http://{}", daemon_bind())
}

/// daemon 日志与运行面目录：`%USERPROFILE%\.browse-rs`。
pub fn state_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".browse-rs")
}

fn client() -> reqwest::Client {
    // 本机回环绝不过系统代理
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("reqwest client")
}

/// daemon 是否在跑（GET /health 通即为在）。
pub async fn daemon_alive() -> Option<Value> {
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(400))
        .build()
        .ok()?;
    let resp = c.get(format!("{}/health", http())).send().await.ok()?;
    resp.json::<Value>().await.ok()
}

/// 确保 daemon 在跑：不通就 detached 拉起 `browse --serve`，再探活。
///
/// # Errors
///
/// 拉起失败（exe 找不到）或 10 秒内 /health 不通。
pub async fn ensure_daemon() -> Result<()> {
    if daemon_alive().await.is_some() {
        return Ok(());
    }
    let exe = std::env::current_exe().context("current_exe")?;
    let dir = state_dir();
    std::fs::create_dir_all(&dir).ok();
    let log = dir.join("daemon.log");
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    let mut cmd = Command::new(exe);
    cmd.args(["--serve", "--bind", &daemon_bind()]);
    let err_out = out.try_clone().ok();
    cmd.stdout(out);
    if let Some(e) = err_out {
        cmd.stderr(e);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("拉起 daemon（日志 {}）", log.display()))?;
    drop(child);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if daemon_alive().await.is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(anyhow!(
        "browse: daemon 10 秒未就绪；看日志 {}（tail 或删除后重跑）",
        log.display()
    ))
}

/// POST /eval。返回片段求值结果；daemon 侧错误进 `Err`（错误串已是给人/agent 的下一步指令形态）。
///
/// # Errors
///
/// HTTP 失败或 daemon 求值失败（语法错、CDP 错、守卫拦截、超时）。
pub async fn eval(code: &str, new_tab: bool) -> Result<Value> {
    let c = client();
    let resp = c
        .post(format!("{}/eval", http()))
        .json(&json!({ "code": code, "new_tab": new_tab }))
        .send()
        .await
        .context("POST /eval")?;
    let body: Value = resp.json().await.context("解析 /eval 响应")?;
    if body.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(body.get("value").cloned().unwrap_or(Value::Null))
    } else {
        // 前缀由 daemon 侧统一加（防双重 browse:）
        Err(anyhow!(
            "{}",
            body.get("error")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        ))
    }
}

/// GET /health。
///
/// # Errors
///
/// daemon 不在或响应坏。
pub async fn health() -> Result<Value> {
    daemon_alive()
        .await
        .ok_or_else(|| anyhow!("browse: daemon 不在（先随便跑一条片段自动拉起，或 browse up）"))
}

/// POST /engine/up（显式起引擎）。
///
/// # Errors
///
/// HTTP 失败或引擎 ensure 失败（连不上、chrome 找不到、spawn 超时）。
pub async fn engine_up(
    headless: bool,
    chrome: Option<String>,
    ws: Option<String>,
    port: Option<u16>,
    pipe: bool,
) -> Result<Value> {
    let c = client();
    let resp = c
        .post(format!("{}/engine/up", http()))
        .json(&json!({ "headless": headless, "chrome": chrome, "ws": ws, "port": port, "pipe": pipe }))
        .send()
        .await
        .context("POST /engine/up")?;
    let body: Value = resp.json().await.context("解析 /engine/up 响应")?;
    if body.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(body)
    } else {
        Err(anyhow!(
            "browse: {}",
            body.get("error")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        ))
    }
}

/// POST /quit（退 daemon；daemon 侧顺带只终结自起引擎）。
///
/// # Errors
///
/// daemon 不在（幂等场景调用方先判活）或 HTTP 失败。
pub async fn quit() -> Result<()> {
    client()
        .post(format!("{}/quit", http()))
        .send()
        .await
        .context("POST /quit")?;
    Ok(())
}
