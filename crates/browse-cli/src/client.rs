//! daemon 客户端：本地 HTTP 调用 + 首次使用自动拉起 detached daemon。

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// daemon 的缺省端口，`BROWSE_PORT` 与 `BROWSE_NAME` 派生端口可覆盖。
///
/// 取 9880 是为避开 bh 的 9876。
pub const DEFAULT_PORT: u16 = 9880;

/// 返回 daemon 监听的 `host:port` 串；多实例由 `BROWSE_NAME` 派生端口（ADR-0006）。
///
/// # Examples
///
/// ```
/// # // BROWSE_PORT/BROWSE_NAME 均未设时才是默认口（CI 干净环境成立）
/// if std::env::var_os("BROWSE_PORT").is_none() && std::env::var_os("BROWSE_NAME").is_none() {
///     assert_eq!(browse_cli::client::daemon_bind(), "127.0.0.1:9880");
/// }
/// ```
pub fn daemon_bind() -> String {
    format!("127.0.0.1:{}", browse_core::paths::daemon_port())
}

fn http() -> String {
    format!("http://{}", daemon_bind())
}

/// 返回 daemon 日志与运行面目录（`%USERPROFILE%\.browse-rs[\<name>]`，多实例各一份）。
pub fn state_dir() -> PathBuf {
    browse_core::paths::state_dir()
}

fn client() -> reqwest::Client {
    // 本机回环绝不过系统代理
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("reqwest client")
}

/// 探测 daemon 是否在跑：GET /health 通即为在。
pub async fn daemon_alive() -> Option<Value> {
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(400))
        .build()
        .ok()?;
    let resp = c.get(format!("{}/health", http())).send().await.ok()?;
    resp.json::<Value>().await.ok()
}

/// daemon 不在跑时 detached 拉起 `browse --serve` 并探活到通；已在跑则直接返回。
///
/// # Errors
///
/// 拉起失败（exe 找不到）或 10 秒内 /health 不通。
pub async fn ensure_daemon() -> Result<()> {
    ensure_daemon_with_env(&[]).await
}

/// 同 [`ensure_daemon`]，但给新拉起的 daemon 进程注入环境变量
/// （#25.1/#25.5：idle-timeout 与代理旗标只影响新 daemon；已在跑的
/// daemon 不动，其口径以启动时为准）。
///
/// # Errors
///
/// daemon 起不来或就绪探测超时。
pub async fn ensure_daemon_with_env(envs: &[(&str, String)]) -> Result<()> {
    if daemon_alive().await.is_some() {
        return Ok(());
    }
    let exe = std::env::current_exe().context("current_exe")?;
    let dir = state_dir();
    std::fs::create_dir_all(&dir).ok();
    let log = dir.join("daemon.log");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        // 先摘掉本进程 stdio 句柄的可继承位：daemon spawn 走
        // bInheritHandles=TRUE（stdio 指到日志文件），而 bash 管道默认
        // 可继承；不摘的话常驻 daemon 会握着调用方 `browse '…' | jq`
        // 的管道写端，管道永不 EOF。显式配置的日志句柄不受影响
        for slot in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = unsafe { GetStdHandle(slot) };
            if !h.is_null() {
                unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0) };
            }
        }
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?;
        let mut cmd = Command::new(exe);
        cmd.envs(envs.iter().map(|(k, v)| (*k, v.clone())));
        cmd.args(["--serve", "--bind", &daemon_bind()]);
        let err_out = out.try_clone().ok();
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(out);
        if let Some(e) = err_out {
            cmd.stderr(e);
        }
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        let child = cmd
            .spawn()
            .with_context(|| format!("拉起 daemon（日志 {}）", log.display()))?;
        drop(child);
    }
    #[cfg(not(windows))]
    {
        // Unix：std 打开的 fd 全带 CLOEXEC，只 dup2 配置过的 stdio，
        // 调用方管道不会被 daemon 带走，直接 spawn 即可
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?;
        let mut cmd = Command::new(exe);
        cmd.envs(envs.iter().map(|(k, v)| (*k, v.clone())));
        cmd.args(["--serve", "--bind", &daemon_bind()]);
        let err_out = out.try_clone().ok();
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(out);
        if let Some(e) = err_out {
            cmd.stderr(e);
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("拉起 daemon（日志 {}）", log.display()))?;
        drop(child);
    }
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

/// 把方言片段 POST 到 daemon 的 /eval 求值。
///
/// 返回最后一条语句的值；daemon 侧错误进 `Err`（错误串已是给人/agent 的下一步指令形态）。
///
/// # Errors
///
/// HTTP 失败或 daemon 求值失败（语法错、CDP 错、守卫拦截、超时）。
pub async fn eval(code: &str, new_tab: bool, js: bool) -> Result<Value> {
    let c = client();
    let resp = c
        .post(format!("{}/eval", http()))
        .json(&json!({ "code": code, "new_tab": new_tab, "js": js }))
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

/// 解码 `-b/--b64` 通道的片段实参（#22）：标准 base64 解码为 UTF-8
/// 字符串。PowerShell 与复杂 shell 的引号与编码面一并绕开（方言与
/// `--js` 两形态通用）。
///
/// # Errors
///
/// 实参不是合法 base64，或解码后不是 UTF-8 文本（错误带生成 base64 的
/// 可照抄命令 CTA）。
///
/// # Examples
///
/// ```
/// assert_eq!(browse_cli::client::decode_arg_b64("cmV0dXJuIDE=").unwrap(), "return 1");
/// // 空白容忍（PowerShell 折行形）
/// assert_eq!(
///     browse_cli::client::decode_arg_b64("cmV0\ndXJuIDE=").unwrap(),
///     "return 1"
/// );
/// assert!(browse_cli::client::decode_arg_b64("!!!").is_err());
/// ```
pub fn decode_arg_b64(s: &str) -> Result<String> {
    // 字母表先行校验：宿主 base64_decode 查表对非法字符得 0（静默出垃圾），
    // 通道面要给出错而不是喂垃圾给引擎
    let alphabet_ok = s.bytes().all(|b| {
        b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' || b.is_ascii_whitespace()
    });
    if !alphabet_ok {
        anyhow::bail!(
            "-b 实参不是 base64 字母表（A-Za-z0-9+/= 与空白）；下一步：base64 -w0 <文件> 生成（PowerShell 用 [Convert]::ToBase64String）"
        );
    }
    let bytes = browse_core::js_host::base64_decode(s).map_err(|e| {
        anyhow!(
            "-b 实参不是合法 base64：{e}；下一步：base64 -w0 <文件> 生成（PowerShell 用 [Convert]::ToBase64String([IO.File]::ReadAllBytes(\"<文件>\"))）"
        )
    })?;
    String::from_utf8(bytes)
        .map_err(|_| anyhow!("-b 解码后不是 UTF-8 文本；下一步：确认原文是方言片段或 JS 源码文本"))
}

/// GET /health 取 daemon 状态面；daemon 不在时报可照抄的拉起提示。
///
/// # Errors
///
/// daemon 不在或响应坏。
pub async fn health() -> Result<Value> {
    daemon_alive()
        .await
        .ok_or_else(|| anyhow!("browse: daemon 不在（先随便跑一条片段自动拉起，或 browse up）"))
}

/// `engine_up` 的参数面：CLI up 旗标直通（字段名即 /engine/up 请求键）。
pub struct UpParams {
    /// spawn 时无头。
    pub headless: bool,
    /// 显式 chrome 路径。
    pub chrome: Option<String>,
    /// 显式 WS URL。
    pub ws: Option<String>,
    /// 显式端口。
    pub port: Option<u16>,
    /// spawn 走 CDP 管道。
    pub pipe: bool,
    /// 自定义 user-data-dir。
    pub profile: Option<String>,
    /// 引擎代理（#25.5）。
    pub proxy: Option<String>,
    /// 代理旁路（#25.5）。
    pub proxy_bypass: Option<String>,
    /// 隔离态 profile（#25.3 拆出）。
    pub isolated: bool,
}

/// POST /engine/up 显式起引擎，走 ensure 全链（附着优先缺则 spawn）。
///
/// # Errors
///
/// HTTP 失败或引擎 ensure 失败（连不上、chrome 找不到、spawn 超时）。
pub async fn engine_up(p: UpParams) -> Result<Value> {
    let c = client();
    let resp = c
        .post(format!("{}/engine/up", http()))
        .json(&json!({
            "headless": p.headless,
            "chrome": p.chrome,
            "ws": p.ws,
            "port": p.port,
            "pipe": p.pipe,
            "profile": p.profile,
            "proxy": p.proxy,
            "proxy_bypass": p.proxy_bypass,
            "isolated": p.isolated,
        }))
        .send()
        .await
        .context("POST /engine/up")?;
    let body: Value = resp.json().await.context("解析 /engine/up 响应")?;
    if body.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(body)
    } else {
        Err(anyhow!(
            "{}",
            body.get("error")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        ))
    }
}

/// POST /quit 退 daemon，daemon 侧顺带只终结自起引擎（附着来源不动）。
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
